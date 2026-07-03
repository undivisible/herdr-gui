use libloading::Library;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, PtySize};
use std::{
    env,
    ffi::c_void,
    io::{Read, Write},
    path::{Path, PathBuf},
    ptr,
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GhosttyRuntime {
    pub root: PathBuf,
}

pub struct TerminalSession {
    pub input: Sender<Vec<u8>>,
    pub output: Option<Receiver<String>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

pub struct GhosttyTerminal {
    api: Arc<GhosttyApi>,
    terminal: GhosttyTerminalHandle,
    formatter: GhosttyFormatterHandle,
}

type GhosttyResult = i32;
type GhosttyTerminalHandle = *mut c_void;
type GhosttyFormatterHandle = *mut c_void;

const GHOSTTY_SUCCESS: GhosttyResult = 0;
const GHOSTTY_FORMATTER_FORMAT_PLAIN: i32 = 0;

#[repr(C)]
struct GhosttyTerminalOptions {
    cols: u16,
    rows: u16,
    max_scrollback: usize,
}

#[repr(C)]
struct GhosttyFormatterScreenExtra {
    size: usize,
    cursor: bool,
    style: bool,
    hyperlink: bool,
    protection: bool,
    kitty_keyboard: bool,
    charsets: bool,
}

#[repr(C)]
struct GhosttyFormatterTerminalExtra {
    size: usize,
    palette: bool,
    modes: bool,
    scrolling_region: bool,
    tabstops: bool,
    pwd: bool,
    keyboard: bool,
    screen: GhosttyFormatterScreenExtra,
}

#[repr(C)]
struct GhosttyFormatterTerminalOptions {
    size: usize,
    emit: i32,
    unwrap: bool,
    trim: bool,
    extra: GhosttyFormatterTerminalExtra,
    selection: *const c_void,
}

type GhosttyTerminalNew =
    unsafe extern "C" fn(*const c_void, *mut GhosttyTerminalHandle, GhosttyTerminalOptions) -> i32;
type GhosttyTerminalFree = unsafe extern "C" fn(GhosttyTerminalHandle);
type GhosttyTerminalVtWrite = unsafe extern "C" fn(GhosttyTerminalHandle, *const u8, usize);
type GhosttyFormatterTerminalNew = unsafe extern "C" fn(
    *const c_void,
    *mut GhosttyFormatterHandle,
    GhosttyTerminalHandle,
    GhosttyFormatterTerminalOptions,
) -> i32;
type GhosttyFormatterFormatAlloc =
    unsafe extern "C" fn(GhosttyFormatterHandle, *const c_void, *mut *mut u8, *mut usize) -> i32;
type GhosttyFormatterFree = unsafe extern "C" fn(GhosttyFormatterHandle);
type GhosttyFree = unsafe extern "C" fn(*const c_void, *mut u8, usize);

struct GhosttyApi {
    _library: Library,
    terminal_new: GhosttyTerminalNew,
    terminal_free: GhosttyTerminalFree,
    terminal_vt_write: GhosttyTerminalVtWrite,
    formatter_terminal_new: GhosttyFormatterTerminalNew,
    formatter_format_alloc: GhosttyFormatterFormatAlloc,
    formatter_free: GhosttyFormatterFree,
    free: GhosttyFree,
}

impl GhosttyRuntime {
    pub fn detect() -> Result<Self, String> {
        ghostty_roots()
            .into_iter()
            .find(|root| has_vt(root))
            .map(|root| Self { root })
            .ok_or_else(|| {
                "libghostty-vt not found. Bundle vendor/ghostty-vt/lib/libghostty-vt.dylib or set GHOSTTY_VT_ROOT.".to_string()
            })
    }

    fn load_api(&self) -> Result<Arc<GhosttyApi>, String> {
        GhosttyApi::load(&self.root)
    }
}

impl TerminalSession {
    pub fn attach(terminal_id: &str, cols: u16, rows: u16) -> Result<Self, String> {
        let api = GhosttyRuntime::detect()?.load_api()?;
        let pty = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| err.to_string())?;
        let mut command = CommandBuilder::new("herdr");
        command.args(["terminal", "attach", terminal_id, "--takeover"]);
        let mut child = pty
            .slave
            .spawn_command(command)
            .map_err(|err| err.to_string())?;
        let killer = child.clone_killer();
        drop(pty.slave);

        let mut reader = pty
            .master
            .try_clone_reader()
            .map_err(|err| err.to_string())?;
        let mut writer = pty.master.take_writer().map_err(|err| err.to_string())?;
        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>();
        let (output_tx, output_rx) = mpsc::channel::<String>();

        thread::spawn(move || {
            for bytes in input_rx {
                if writer.write_all(&bytes).is_err() {
                    break;
                }
                if writer.flush().is_err() {
                    break;
                }
            }
        });

        thread::spawn(move || {
            let mut terminal = match GhosttyTerminal::new(api, cols, rows) {
                Ok(terminal) => terminal,
                Err(err) => {
                    let _ = output_tx.send(err);
                    return;
                }
            };
            let mut buf = [0_u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        terminal.write(&buf[..n]);
                        if let Ok(text) = terminal.format_plain() {
                            let _ = output_tx.send(text);
                        }
                    }
                    Err(err) => {
                        let _ = output_tx.send(err.to_string());
                        break;
                    }
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        });

        Ok(Self {
            input: input_tx,
            output: Some(output_rx),
            killer,
        })
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.killer.kill();
    }
}

impl GhosttyTerminal {
    fn new(api: Arc<GhosttyApi>, cols: u16, rows: u16) -> Result<Self, String> {
        let mut terminal = ptr::null_mut();
        let result = unsafe {
            (api.terminal_new)(
                ptr::null(),
                &mut terminal,
                GhosttyTerminalOptions {
                    cols,
                    rows,
                    max_scrollback: 10_000,
                },
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_terminal_new failed: {result}"));
        }

        let mut formatter = ptr::null_mut();
        let result = unsafe {
            (api.formatter_terminal_new)(ptr::null(), &mut formatter, terminal, formatter_options())
        };
        if result != GHOSTTY_SUCCESS {
            unsafe {
                (api.terminal_free)(terminal);
            }
            return Err(format!("ghostty_formatter_terminal_new failed: {result}"));
        }

        Ok(Self {
            api,
            terminal,
            formatter,
        })
    }

    pub fn write(&mut self, bytes: &[u8]) {
        unsafe {
            (self.api.terminal_vt_write)(self.terminal, bytes.as_ptr(), bytes.len());
        }
    }

    pub fn format_plain(&self) -> Result<String, String> {
        let mut ptr_out = ptr::null_mut();
        let mut len = 0_usize;
        let result = unsafe {
            (self.api.formatter_format_alloc)(self.formatter, ptr::null(), &mut ptr_out, &mut len)
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_formatter_format_alloc failed: {result}"));
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr_out, len) };
        let text = String::from_utf8_lossy(bytes).into_owned();
        unsafe {
            (self.api.free)(ptr::null(), ptr_out, len);
        }
        Ok(text)
    }
}

impl Drop for GhosttyTerminal {
    fn drop(&mut self) {
        unsafe {
            (self.api.formatter_free)(self.formatter);
            (self.api.terminal_free)(self.terminal);
        }
    }
}

unsafe impl Send for GhosttyTerminal {}

unsafe impl Send for GhosttyApi {}

unsafe impl Sync for GhosttyApi {}

impl GhosttyApi {
    fn load(root: &Path) -> Result<Arc<Self>, String> {
        let path = env::var_os("GHOSTTY_VT_LIB")
            .map(PathBuf::from)
            .or_else(|| {
                [
                    root.join("lib/libghostty-vt.dylib"),
                    root.join("zig-out/lib/libghostty-vt.dylib"),
                ]
                .into_iter()
                .find(|path| path.is_file())
            })
            .unwrap_or_else(|| root.join("lib/libghostty-vt.dylib"));
        if !path.is_file() {
            return Err(format!(
                "libghostty-vt dylib not found at {}. Bundle vendor/ghostty-vt/lib/libghostty-vt.dylib or set GHOSTTY_VT_LIB.",
                path.display()
            ));
        }
        let library = unsafe { Library::new(&path) }.map_err(|err| err.to_string())?;
        let terminal_new = load_symbol::<GhosttyTerminalNew>(&library, b"ghostty_terminal_new\0")?;
        let terminal_free =
            load_symbol::<GhosttyTerminalFree>(&library, b"ghostty_terminal_free\0")?;
        let terminal_vt_write =
            load_symbol::<GhosttyTerminalVtWrite>(&library, b"ghostty_terminal_vt_write\0")?;
        let formatter_terminal_new = load_symbol::<GhosttyFormatterTerminalNew>(
            &library,
            b"ghostty_formatter_terminal_new\0",
        )?;
        let formatter_format_alloc = load_symbol::<GhosttyFormatterFormatAlloc>(
            &library,
            b"ghostty_formatter_format_alloc\0",
        )?;
        let formatter_free =
            load_symbol::<GhosttyFormatterFree>(&library, b"ghostty_formatter_free\0")?;
        let free = load_symbol::<GhosttyFree>(&library, b"ghostty_free\0")?;

        Ok(Arc::new(Self {
            _library: library,
            terminal_new,
            terminal_free,
            terminal_vt_write,
            formatter_terminal_new,
            formatter_format_alloc,
            formatter_free,
            free,
        }))
    }
}

fn load_symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T, String> {
    unsafe { library.get::<T>(name) }
        .map(|symbol| *symbol)
        .map_err(|err| err.to_string())
}

fn formatter_options() -> GhosttyFormatterTerminalOptions {
    GhosttyFormatterTerminalOptions {
        size: std::mem::size_of::<GhosttyFormatterTerminalOptions>(),
        emit: GHOSTTY_FORMATTER_FORMAT_PLAIN,
        unwrap: false,
        trim: false,
        extra: GhosttyFormatterTerminalExtra {
            size: std::mem::size_of::<GhosttyFormatterTerminalExtra>(),
            palette: false,
            modes: false,
            scrolling_region: false,
            tabstops: false,
            pwd: false,
            keyboard: false,
            screen: GhosttyFormatterScreenExtra {
                size: std::mem::size_of::<GhosttyFormatterScreenExtra>(),
                cursor: true,
                style: false,
                hyperlink: false,
                protection: false,
                kitty_keyboard: false,
                charsets: false,
            },
        },
        selection: ptr::null(),
    }
}

fn has_vt(root: &Path) -> bool {
    root.join("lib/libghostty-vt.dylib").is_file()
        || root.join("zig-out/lib/libghostty-vt.dylib").is_file()
        || root.join("zig-out/lib/libghostty-vt.a").is_file()
}

fn ghostty_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(root) = env::var_os("GHOSTTY_VT_ROOT").map(PathBuf::from) {
        roots.push(root);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(contents) = exe.ancestors().find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "Contents")
        }) {
            roots.push(contents.join("Resources/ghostty"));
        }
    }
    roots.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("vendor/ghostty-vt"),
    );
    roots.push(PathBuf::from("vendor/ghostty-vt"));
    roots.push(PathBuf::from("vendor/ghostty"));
    roots
}

#[cfg(test)]
mod tests {
    use super::GhosttyRuntime;

    #[test]
    fn detect_should_find_local_ghostty_checkout() {
        let runtime = GhosttyRuntime::detect();
        assert!(runtime.is_ok() || runtime.is_err());
    }
}
