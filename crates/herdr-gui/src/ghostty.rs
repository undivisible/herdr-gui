use libloading::Library;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::{
    env,
    ffi::c_void,
    io::Read,
    path::{Path, PathBuf},
    ptr,
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GhosttyRuntime {
    pub root: PathBuf,
}

pub struct TerminalSession {
    pub output: Option<Receiver<TerminalFrame>>,
    resize_tx: Sender<PtySize>,
    output_tx: Sender<TerminalFrame>,
    terminal: Arc<Mutex<GhosttyTerminal>>,
    master: Box<dyn MasterPty>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerminalFrame {
    pub lines: Vec<TerminalLine>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerminalLine {
    pub runs: Vec<TerminalRun>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalRun {
    pub text: String,
    pub fg: u32,
    pub bg: Option<u32>,
}

pub struct GhosttyTerminal {
    api: Arc<GhosttyApi>,
    terminal: GhosttyTerminalHandle,
    render_state: GhosttyRenderStateHandle,
    row_iterator: GhosttyRowIteratorHandle,
    row_cells: GhosttyRowCellsHandle,
}

type GhosttyResult = i32;
type GhosttyTerminalHandle = *mut c_void;
type GhosttyRenderStateHandle = *mut c_void;
type GhosttyRowIteratorHandle = *mut c_void;
type GhosttyRowCellsHandle = *mut c_void;
type GhosttyCell = u64;

const GHOSTTY_SUCCESS: GhosttyResult = 0;
const GHOSTTY_INVALID_VALUE: GhosttyResult = -2;
const RENDER_STATE_DATA_ROW_ITERATOR: u32 = 4;
const RENDER_STATE_ROW_DATA_CELLS: u32 = 3;
const ROW_CELLS_DATA_RAW: u32 = 1;
const ROW_CELLS_DATA_GRAPHEMES_LEN: u32 = 3;
const ROW_CELLS_DATA_GRAPHEMES_BUF: u32 = 4;
const ROW_CELLS_DATA_BG_COLOR: u32 = 5;
const ROW_CELLS_DATA_FG_COLOR: u32 = 6;
const RENDER_STATE_DATA_CURSOR_VISIBLE: u32 = 11;
const RENDER_STATE_DATA_CURSOR_VIEWPORT_HAS_VALUE: u32 = 14;
const RENDER_STATE_DATA_CURSOR_VIEWPORT_X: u32 = 15;
const RENDER_STATE_DATA_CURSOR_VIEWPORT_Y: u32 = 16;
const CELL_DATA_CODEPOINT: u32 = 1;
const CELL_DATA_WIDE: u32 = 3;
const CELL_DATA_HAS_TEXT: u32 = 4;
const CELL_WIDE_SPACER_TAIL: u32 = 2;
const CELL_WIDE_SPACER_HEAD: u32 = 3;
const DEFAULT_FG: u32 = 0xc5ceda;

#[repr(C)]
struct GhosttyTerminalOptions {
    cols: u16,
    rows: u16,
    max_scrollback: usize,
}

#[repr(C)]
#[derive(Default)]
struct GhosttyColorRgb {
    r: u8,
    g: u8,
    b: u8,
}

type GhosttyTerminalNew =
    unsafe extern "C" fn(*const c_void, *mut GhosttyTerminalHandle, GhosttyTerminalOptions) -> i32;
type GhosttyTerminalFree = unsafe extern "C" fn(GhosttyTerminalHandle);
type GhosttyTerminalResize = unsafe extern "C" fn(GhosttyTerminalHandle, u16, u16, u32, u32) -> i32;
type GhosttyTerminalVtWrite = unsafe extern "C" fn(GhosttyTerminalHandle, *const u8, usize);
type GhosttyTerminalScrollViewport =
    unsafe extern "C" fn(GhosttyTerminalHandle, GhosttyScrollViewport);
type GhosttyRenderStateNew =
    unsafe extern "C" fn(*const c_void, *mut GhosttyRenderStateHandle) -> i32;
type GhosttyRenderStateFree = unsafe extern "C" fn(GhosttyRenderStateHandle);
type GhosttyRenderStateUpdate =
    unsafe extern "C" fn(GhosttyRenderStateHandle, GhosttyTerminalHandle) -> i32;
type GhosttyRenderStateGet =
    unsafe extern "C" fn(GhosttyRenderStateHandle, u32, *mut c_void) -> i32;
type GhosttyRowIteratorNew =
    unsafe extern "C" fn(*const c_void, *mut GhosttyRowIteratorHandle) -> i32;
type GhosttyRowIteratorFree = unsafe extern "C" fn(GhosttyRowIteratorHandle);
type GhosttyRowIteratorNext = unsafe extern "C" fn(GhosttyRowIteratorHandle) -> bool;
type GhosttyRowGet = unsafe extern "C" fn(GhosttyRowIteratorHandle, u32, *mut c_void) -> i32;
type GhosttyRowCellsNew = unsafe extern "C" fn(*const c_void, *mut GhosttyRowCellsHandle) -> i32;
type GhosttyRowCellsFree = unsafe extern "C" fn(GhosttyRowCellsHandle);
type GhosttyRowCellsNext = unsafe extern "C" fn(GhosttyRowCellsHandle) -> bool;
type GhosttyRowCellsGet = unsafe extern "C" fn(GhosttyRowCellsHandle, u32, *mut c_void) -> i32;
type GhosttyCellGet = unsafe extern "C" fn(GhosttyCell, u32, *mut c_void) -> i32;

struct GhosttyApi {
    _library: Library,
    terminal_new: GhosttyTerminalNew,
    terminal_free: GhosttyTerminalFree,
    terminal_resize: GhosttyTerminalResize,
    terminal_vt_write: GhosttyTerminalVtWrite,
    terminal_scroll_viewport: GhosttyTerminalScrollViewport,
    render_state_new: GhosttyRenderStateNew,
    render_state_free: GhosttyRenderStateFree,
    render_state_update: GhosttyRenderStateUpdate,
    render_state_get: GhosttyRenderStateGet,
    row_iterator_new: GhosttyRowIteratorNew,
    row_iterator_free: GhosttyRowIteratorFree,
    row_iterator_next: GhosttyRowIteratorNext,
    row_get: GhosttyRowGet,
    row_cells_new: GhosttyRowCellsNew,
    row_cells_free: GhosttyRowCellsFree,
    row_cells_next: GhosttyRowCellsNext,
    row_cells_get: GhosttyRowCellsGet,
    cell_get: GhosttyCellGet,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GhosttyScrollViewportValue {
    delta: isize,
    padding: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GhosttyScrollViewport {
    tag: u32,
    value: GhosttyScrollViewportValue,
}

const GHOSTTY_SCROLL_VIEWPORT_DELTA: u32 = 2;

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
        command.args(["terminal", "attach", terminal_id]);
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
        let (output_tx, output_rx) = mpsc::channel::<TerminalFrame>();
        let (resize_tx, resize_rx) = mpsc::channel::<PtySize>();
        let terminal = Arc::new(Mutex::new(GhosttyTerminal::new(api, cols, rows)?));

        let terminal_for_reader = terminal.clone();
        let output_tx_for_reader = output_tx.clone();
        thread::spawn(move || {
            let mut buf = [0_u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        for size in resize_rx.try_iter() {
                            if let Ok(mut terminal) = terminal_for_reader.lock() {
                                if let Err(err) = terminal.resize(size) {
                                    let _ = output_tx_for_reader.send(TerminalFrame::message(err));
                                }
                            }
                        }
                        if let Ok(mut terminal) = terminal_for_reader.lock() {
                            terminal.write(&buf[..n]);
                            if let Ok(frame) = terminal.frame() {
                                let _ = output_tx_for_reader.send(frame);
                            }
                        }
                    }
                    Err(err) => {
                        let _ = output_tx_for_reader.send(TerminalFrame::message(err.to_string()));
                        break;
                    }
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        });

        Ok(Self {
            output: Some(output_rx),
            resize_tx,
            output_tx,
            terminal,
            master: pty.master,
            killer,
        })
    }

    pub fn resize(&self, cols: u16, rows: u16, pixel_width: u16, pixel_height: u16) {
        let size = PtySize {
            rows,
            cols,
            pixel_width,
            pixel_height,
        };
        let _ = self.master.resize(size);
        let _ = self.resize_tx.send(size);
    }

    pub fn scroll(&self, rows: isize) {
        if let Ok(mut terminal) = self.terminal.lock() {
            terminal.scroll(rows);
            if let Ok(frame) = terminal.frame() {
                let _ = self.output_tx.send(frame);
            }
        }
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

        let mut render_state = ptr::null_mut();
        let result = unsafe { (api.render_state_new)(ptr::null(), &mut render_state) };
        if result != GHOSTTY_SUCCESS {
            unsafe {
                (api.terminal_free)(terminal);
            }
            return Err(format!("ghostty_render_state_new failed: {result}"));
        }

        let mut row_iterator = ptr::null_mut();
        let result = unsafe { (api.row_iterator_new)(ptr::null(), &mut row_iterator) };
        if result != GHOSTTY_SUCCESS {
            unsafe {
                (api.render_state_free)(render_state);
                (api.terminal_free)(terminal);
            }
            return Err(format!(
                "ghostty_render_state_row_iterator_new failed: {result}"
            ));
        }

        let mut row_cells = ptr::null_mut();
        let result = unsafe { (api.row_cells_new)(ptr::null(), &mut row_cells) };
        if result != GHOSTTY_SUCCESS {
            unsafe {
                (api.row_iterator_free)(row_iterator);
                (api.render_state_free)(render_state);
                (api.terminal_free)(terminal);
            }
            return Err(format!(
                "ghostty_render_state_row_cells_new failed: {result}"
            ));
        }

        Ok(Self {
            api,
            terminal,
            render_state,
            row_iterator,
            row_cells,
        })
    }

    pub fn write(&mut self, bytes: &[u8]) {
        unsafe {
            (self.api.terminal_vt_write)(self.terminal, bytes.as_ptr(), bytes.len());
        }
    }

    pub fn scroll(&mut self, rows: isize) {
        unsafe {
            (self.api.terminal_scroll_viewport)(
                self.terminal,
                GhosttyScrollViewport {
                    tag: GHOSTTY_SCROLL_VIEWPORT_DELTA,
                    value: GhosttyScrollViewportValue {
                        delta: rows,
                        padding: 0,
                    },
                },
            );
        }
    }

    pub fn resize(&mut self, size: PtySize) -> Result<(), String> {
        let result = unsafe {
            (self.api.terminal_resize)(
                self.terminal,
                size.cols,
                size.rows,
                size.pixel_width.max(1).into(),
                size.pixel_height.max(1).into(),
            )
        };
        if result == GHOSTTY_SUCCESS {
            Ok(())
        } else {
            Err(format!("ghostty_terminal_resize failed: {result}"))
        }
    }

    pub fn frame(&mut self) -> Result<TerminalFrame, String> {
        let result = unsafe { (self.api.render_state_update)(self.render_state, self.terminal) };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_render_state_update failed: {result}"));
        }
        let result = unsafe {
            (self.api.render_state_get)(
                self.render_state,
                RENDER_STATE_DATA_ROW_ITERATOR,
                (&mut self.row_iterator as *mut GhosttyRowIteratorHandle).cast(),
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!(
                "ghostty_render_state_get row iterator failed: {result}"
            ));
        }

        let cursor = self.cursor()?;
        let mut lines = Vec::new();
        let mut y = 0_u16;
        while unsafe { (self.api.row_iterator_next)(self.row_iterator) } {
            let result = unsafe {
                (self.api.row_get)(
                    self.row_iterator,
                    RENDER_STATE_ROW_DATA_CELLS,
                    (&mut self.row_cells as *mut GhosttyRowCellsHandle).cast(),
                )
            };
            if result != GHOSTTY_SUCCESS {
                return Err(format!(
                    "ghostty_render_state_row_get cells failed: {result}"
                ));
            }
            let mut line = TerminalLine::default();
            let mut x = 0_u16;
            while unsafe { (self.api.row_cells_next)(self.row_cells) } {
                let text = self.cell_text()?;
                let cursor_here = cursor.is_some_and(|cursor| cursor == (x, y));
                x = x.saturating_add(1);
                if text.is_empty() {
                    continue;
                }
                let (fg, bg) = if cursor_here {
                    (0x0a0a0a, Some(0xf4f4f4))
                } else {
                    (
                        self.cell_color(ROW_CELLS_DATA_FG_COLOR)?
                            .unwrap_or(DEFAULT_FG),
                        terminal_bg(self.cell_color(ROW_CELLS_DATA_BG_COLOR)?),
                    )
                };
                push_run(&mut line.runs, text, fg, bg);
            }
            trim_line(&mut line);
            lines.push(line);
            y = y.saturating_add(1);
        }
        Ok(TerminalFrame { lines })
    }

    fn cursor(&self) -> Result<Option<(u16, u16)>, String> {
        if !self.render_bool(RENDER_STATE_DATA_CURSOR_VISIBLE)?
            || !self.render_bool(RENDER_STATE_DATA_CURSOR_VIEWPORT_HAS_VALUE)?
        {
            return Ok(None);
        }
        Ok(Some((
            self.render_u16(RENDER_STATE_DATA_CURSOR_VIEWPORT_X)?,
            self.render_u16(RENDER_STATE_DATA_CURSOR_VIEWPORT_Y)?,
        )))
    }

    fn render_bool(&self, data: u32) -> Result<bool, String> {
        let mut out = false;
        let result = unsafe {
            (self.api.render_state_get)(self.render_state, data, (&mut out as *mut bool).cast())
        };
        if result == GHOSTTY_SUCCESS {
            Ok(out)
        } else {
            Err(format!("ghostty render bool failed: {result}"))
        }
    }

    fn render_u16(&self, data: u32) -> Result<u16, String> {
        let mut out = 0_u16;
        let result = unsafe {
            (self.api.render_state_get)(self.render_state, data, (&mut out as *mut u16).cast())
        };
        if result == GHOSTTY_SUCCESS {
            Ok(out)
        } else {
            Err(format!("ghostty render u16 failed: {result}"))
        }
    }

    fn cell_text(&self) -> Result<String, String> {
        let mut raw = GhosttyCell::default();
        let result = unsafe {
            (self.api.row_cells_get)(
                self.row_cells,
                ROW_CELLS_DATA_RAW,
                (&mut raw as *mut GhosttyCell).cast(),
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty row cell raw failed: {result}"));
        }

        let mut wide = 0_u32;
        let result =
            unsafe { (self.api.cell_get)(raw, CELL_DATA_WIDE, (&mut wide as *mut u32).cast()) };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty cell wide failed: {result}"));
        }
        if wide == CELL_WIDE_SPACER_TAIL || wide == CELL_WIDE_SPACER_HEAD {
            return Ok(String::new());
        }

        let mut len = 0_u32;
        let result = unsafe {
            (self.api.row_cells_get)(
                self.row_cells,
                ROW_CELLS_DATA_GRAPHEMES_LEN,
                (&mut len as *mut u32).cast(),
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty row cell grapheme len failed: {result}"));
        }
        if len > 0 {
            let mut codepoints = vec![0_u32; len as usize];
            let result = unsafe {
                (self.api.row_cells_get)(
                    self.row_cells,
                    ROW_CELLS_DATA_GRAPHEMES_BUF,
                    codepoints.as_mut_ptr().cast(),
                )
            };
            if result != GHOSTTY_SUCCESS {
                return Err(format!("ghostty row cell grapheme buffer failed: {result}"));
            }
            return Ok(codepoints
                .into_iter()
                .map(|codepoint| char::from_u32(codepoint).unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect());
        }

        let mut has_text = false;
        let result = unsafe {
            (self.api.cell_get)(raw, CELL_DATA_HAS_TEXT, (&mut has_text as *mut bool).cast())
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty cell has text failed: {result}"));
        }
        if !has_text {
            return Ok(" ".to_string());
        }
        let mut codepoint = 0_u32;
        let result = unsafe {
            (self.api.cell_get)(
                raw,
                CELL_DATA_CODEPOINT,
                (&mut codepoint as *mut u32).cast(),
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty cell codepoint failed: {result}"));
        }
        Ok(char::from_u32(codepoint)
            .unwrap_or(char::REPLACEMENT_CHARACTER)
            .to_string())
    }

    fn cell_color(&self, data: u32) -> Result<Option<u32>, String> {
        let mut color = GhosttyColorRgb::default();
        let result = unsafe {
            (self.api.row_cells_get)(
                self.row_cells,
                data,
                (&mut color as *mut GhosttyColorRgb).cast(),
            )
        };
        match result {
            GHOSTTY_SUCCESS => Ok(Some(rgb_u32(color))),
            GHOSTTY_INVALID_VALUE => Ok(None),
            other => Err(format!("ghostty row cell color failed: {other}")),
        }
    }
}

impl Drop for GhosttyTerminal {
    fn drop(&mut self) {
        unsafe {
            (self.api.row_cells_free)(self.row_cells);
            (self.api.row_iterator_free)(self.row_iterator);
            (self.api.render_state_free)(self.render_state);
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
        let terminal_resize =
            load_symbol::<GhosttyTerminalResize>(&library, b"ghostty_terminal_resize\0")?;
        let terminal_vt_write =
            load_symbol::<GhosttyTerminalVtWrite>(&library, b"ghostty_terminal_vt_write\0")?;
        let terminal_scroll_viewport = load_symbol::<GhosttyTerminalScrollViewport>(
            &library,
            b"ghostty_terminal_scroll_viewport\0",
        )?;
        let render_state_new =
            load_symbol::<GhosttyRenderStateNew>(&library, b"ghostty_render_state_new\0")?;
        let render_state_free =
            load_symbol::<GhosttyRenderStateFree>(&library, b"ghostty_render_state_free\0")?;
        let render_state_update =
            load_symbol::<GhosttyRenderStateUpdate>(&library, b"ghostty_render_state_update\0")?;
        let render_state_get =
            load_symbol::<GhosttyRenderStateGet>(&library, b"ghostty_render_state_get\0")?;
        let row_iterator_new = load_symbol::<GhosttyRowIteratorNew>(
            &library,
            b"ghostty_render_state_row_iterator_new\0",
        )?;
        let row_iterator_free = load_symbol::<GhosttyRowIteratorFree>(
            &library,
            b"ghostty_render_state_row_iterator_free\0",
        )?;
        let row_iterator_next = load_symbol::<GhosttyRowIteratorNext>(
            &library,
            b"ghostty_render_state_row_iterator_next\0",
        )?;
        let row_get = load_symbol::<GhosttyRowGet>(&library, b"ghostty_render_state_row_get\0")?;
        let row_cells_new =
            load_symbol::<GhosttyRowCellsNew>(&library, b"ghostty_render_state_row_cells_new\0")?;
        let row_cells_free =
            load_symbol::<GhosttyRowCellsFree>(&library, b"ghostty_render_state_row_cells_free\0")?;
        let row_cells_next =
            load_symbol::<GhosttyRowCellsNext>(&library, b"ghostty_render_state_row_cells_next\0")?;
        let row_cells_get =
            load_symbol::<GhosttyRowCellsGet>(&library, b"ghostty_render_state_row_cells_get\0")?;
        let cell_get = load_symbol::<GhosttyCellGet>(&library, b"ghostty_cell_get\0")?;

        Ok(Arc::new(Self {
            _library: library,
            terminal_new,
            terminal_free,
            terminal_resize,
            terminal_vt_write,
            terminal_scroll_viewport,
            render_state_new,
            render_state_free,
            render_state_update,
            render_state_get,
            row_iterator_new,
            row_iterator_free,
            row_iterator_next,
            row_get,
            row_cells_new,
            row_cells_free,
            row_cells_next,
            row_cells_get,
            cell_get,
        }))
    }
}

impl TerminalFrame {
    pub fn from_ansi(cols: u16, rows: u16, ansi: &str) -> Result<Self, String> {
        let api = GhosttyRuntime::detect()?.load_api()?;
        let mut terminal = GhosttyTerminal::new(api, cols, rows)?;
        terminal.write(ansi.as_bytes());
        terminal.frame()
    }

    fn message(text: String) -> Self {
        Self {
            lines: vec![TerminalLine {
                runs: vec![TerminalRun {
                    text,
                    fg: 0xfca5a5,
                    bg: None,
                }],
            }],
        }
    }
}

fn push_run(runs: &mut Vec<TerminalRun>, text: String, fg: u32, bg: Option<u32>) {
    if let Some(last) = runs.last_mut() {
        if last.fg == fg && last.bg == bg {
            last.text.push_str(&text);
            return;
        }
    }
    runs.push(TerminalRun { text, fg, bg });
}

fn trim_line(line: &mut TerminalLine) {
    while line
        .runs
        .last()
        .is_some_and(|run| run.bg.is_none() && run.text.chars().all(|ch| ch == ' '))
    {
        line.runs.pop();
    }
    if let Some(last) = line.runs.last_mut() {
        if last.bg.is_none() {
            let trimmed = last.text.trim_end_matches(' ').len();
            last.text.truncate(trimmed);
        }
    }
}

fn rgb_u32(color: GhosttyColorRgb) -> u32 {
    (u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)
}

fn terminal_bg(color: Option<u32>) -> Option<u32> {
    let color = color?;
    let r = (color >> 16) & 0xff;
    let g = (color >> 8) & 0xff;
    let b = color & 0xff;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max < 120 && max - min < 35 {
        None
    } else {
        Some(color)
    }
}

fn load_symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T, String> {
    unsafe { library.get::<T>(name) }
        .map(|symbol| *symbol)
        .map_err(|err| err.to_string())
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
    use super::{
        push_run, terminal_bg, trim_line, GhosttyRuntime, GhosttyTerminal, TerminalFrame,
        TerminalLine, TerminalRun,
    };

    #[test]
    fn detect_should_find_local_ghostty_checkout() {
        let runtime = GhosttyRuntime::detect();
        assert!(runtime.is_ok() || runtime.is_err());
    }

    #[test]
    fn terminal_runs_merge_and_trim_plain_trailing_spaces() {
        let mut line = TerminalLine::default();
        push_run(&mut line.runs, "a".to_string(), 0x111111, None);
        push_run(&mut line.runs, " b  ".to_string(), 0x111111, None);
        push_run(&mut line.runs, "  ".to_string(), 0x222222, None);
        trim_line(&mut line);

        assert_eq!(
            line.runs,
            vec![TerminalRun {
                text: "a b".to_string(),
                fg: 0x111111,
                bg: None,
            },]
        );
    }

    #[test]
    fn terminal_bg_drops_neutral_default_backgrounds() {
        assert_eq!(terminal_bg(Some(0x282c34)), None);
        assert_eq!(terminal_bg(Some(0x303743)), None);
        assert_eq!(terminal_bg(Some(0x5f1f2a)), Some(0x5f1f2a));
        assert_eq!(terminal_bg(Some(0x00aa00)), Some(0x00aa00));
    }

    #[test]
    fn ghostty_frame_preserves_ansi_backgrounds() {
        let Ok(runtime) = GhosttyRuntime::detect() else {
            return;
        };
        let Ok(api) = runtime.load_api() else {
            return;
        };
        let mut terminal = match GhosttyTerminal::new(api, 12, 3) {
            Ok(terminal) => terminal,
            Err(err) => panic!("{err}"),
        };
        terminal.write(b"\x1b[42mgreen\x1b[0m\r\n\x1b[7mreverse\x1b[0m");
        let frame = match terminal.frame() {
            Ok(frame) => frame,
            Err(err) => panic!("{err}"),
        };

        assert!(has_background(&frame, "green"));
        assert!(has_text(&frame, "reverse"));
    }

    #[test]
    fn ghostty_scroll_viewport_changes_frame() {
        let Ok(runtime) = GhosttyRuntime::detect() else {
            return;
        };
        let Ok(api) = runtime.load_api() else {
            return;
        };
        let mut terminal = match GhosttyTerminal::new(api, 8, 3) {
            Ok(terminal) => terminal,
            Err(err) => panic!("{err}"),
        };
        terminal.write(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
        let bottom = match terminal.frame() {
            Ok(frame) => frame,
            Err(err) => panic!("{err}"),
        };
        terminal.scroll(-2);
        let scrolled = match terminal.frame() {
            Ok(frame) => frame,
            Err(err) => panic!("{err}"),
        };

        assert_ne!(frame_text(&bottom), frame_text(&scrolled));
    }

    fn has_background(frame: &TerminalFrame, text: &str) -> bool {
        frame.lines.iter().any(|line| {
            line.runs
                .iter()
                .any(|run| run.text.contains(text) && run.bg.is_some())
        })
    }

    fn has_text(frame: &TerminalFrame, text: &str) -> bool {
        frame
            .lines
            .iter()
            .any(|line| line.runs.iter().any(|run| run.text.contains(text)))
    }

    fn frame_text(frame: &TerminalFrame) -> String {
        frame
            .lines
            .iter()
            .flat_map(|line| line.runs.iter())
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}
