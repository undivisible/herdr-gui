use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::{
    ffi::c_void,
    ptr,
    sync::{
        mpsc::{self, Receiver},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GhosttyRuntime;

pub struct TerminalSession {
    pub output: Option<Receiver<Vec<u8>>>,
    terminal: GhosttyTerminal,
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
    /// Inclusive viewport column range selected on this row.
    pub selection: Option<(u16, u16)>,
    /// One entry per grid cell, including blank cells and wide spacers.
    pub cells: Vec<TerminalCell>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalCell {
    pub column: u16,
    /// Glyph width in columns; spacer entries still occupy one grid cell.
    pub span: u16,
    pub kind: TerminalCellKind,
    pub text: String,
    pub fg: u32,
    pub bg: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalCellKind {
    Narrow,
    Wide,
    SpacerTail,
    SpacerHead,
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
    drag_anchor: Option<DragAnchor>,
}

struct DragAnchor {
    handle: *mut c_void,
    screen: u32,
}

impl Drop for DragAnchor {
    fn drop(&mut self) {
        unsafe { ghostty_tracked_grid_ref_free(self.handle) }
    }
}

type GhosttyResult = i32;
type GhosttyTerminalHandle = *mut c_void;
type GhosttyRenderStateHandle = *mut c_void;
type GhosttyRowIteratorHandle = *mut c_void;
type GhosttyRowCellsHandle = *mut c_void;
type GhosttyCell = u64;

const GHOSTTY_SUCCESS: GhosttyResult = 0;
const GHOSTTY_INVALID_VALUE: GhosttyResult = -2;
const GHOSTTY_NO_VALUE: GhosttyResult = -4;
const GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN: u32 = 6;
const GHOSTTY_TERMINAL_DATA_COLOR_BACKGROUND_DEFAULT: u32 = 23;
const RENDER_STATE_DATA_ROW_ITERATOR: u32 = 4;
const RENDER_STATE_ROW_DATA_CELLS: u32 = 3;
const RENDER_STATE_ROW_DATA_SELECTION: u32 = 4;
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
const CELL_WIDE_NARROW: u32 = 0;
const CELL_WIDE_WIDE: u32 = 1;
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

#[repr(C)]
#[derive(Clone, Copy)]
struct GridRef {
    size: usize,
    node: *mut c_void,
    x: u16,
    y: u16,
}

impl Default for GridRef {
    fn default() -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            node: ptr::null_mut(),
            x: 0,
            y: 0,
        }
    }
}

#[repr(C)]
#[derive(Default)]
struct Selection {
    size: usize,
    start: GridRef,
    end: GridRef,
    rectangle: bool,
}

impl Selection {
    fn new() -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            ..Self::default()
        }
    }
}

#[repr(C)]
struct PointCoordinate {
    x: u16,
    y: u32,
}

#[repr(C)]
union PointValue {
    coordinate: std::mem::ManuallyDrop<PointCoordinate>,
    padding: [u64; 2],
}

#[repr(C)]
struct Point {
    tag: u32,
    value: PointValue,
}

impl Point {
    fn viewport(x: u16, y: u16) -> Self {
        Self {
            tag: 1,
            value: PointValue {
                coordinate: std::mem::ManuallyDrop::new(PointCoordinate { x, y: y.into() }),
            },
        }
    }
}

#[repr(C)]
struct SelectionFormatOptions {
    size: usize,
    emit: u32,
    unwrap: bool,
    trim: bool,
    selection: *const Selection,
}

#[repr(C)]
struct RowSelection {
    size: usize,
    start_x: u16,
    end_x: u16,
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

extern "C" {
    fn ghostty_terminal_grid_ref_track(
        terminal: GhosttyTerminalHandle,
        point: Point,
        out: *mut *mut c_void,
    ) -> i32;
    fn ghostty_tracked_grid_ref_snapshot(reference: *mut c_void, out: *mut GridRef) -> i32;
    fn ghostty_tracked_grid_ref_free(reference: *mut c_void);
    fn ghostty_terminal_grid_ref(
        terminal: GhosttyTerminalHandle,
        point: Point,
        out: *mut GridRef,
    ) -> i32;
    fn ghostty_terminal_set(
        terminal: GhosttyTerminalHandle,
        option: u32,
        value: *const c_void,
    ) -> i32;
    fn ghostty_terminal_select_all(terminal: GhosttyTerminalHandle, out: *mut Selection) -> i32;
    fn ghostty_terminal_selection_format_buf(
        terminal: GhosttyTerminalHandle,
        options: SelectionFormatOptions,
        buf: *mut u8,
        len: usize,
        written: *mut usize,
    ) -> i32;
    fn ghostty_terminal_new(
        config: *const c_void,
        terminal: *mut GhosttyTerminalHandle,
        options: GhosttyTerminalOptions,
    ) -> i32;
    fn ghostty_terminal_free(terminal: GhosttyTerminalHandle);
    fn ghostty_terminal_resize(
        terminal: GhosttyTerminalHandle,
        cols: u16,
        rows: u16,
        cell_width: u32,
        cell_height: u32,
    ) -> i32;
    fn ghostty_terminal_vt_write(terminal: GhosttyTerminalHandle, data: *const u8, len: usize);
    fn ghostty_terminal_scroll_viewport(
        terminal: GhosttyTerminalHandle,
        behavior: GhosttyScrollViewport,
    );
    fn ghostty_render_state_new(
        config: *const c_void,
        render_state: *mut GhosttyRenderStateHandle,
    ) -> i32;
    fn ghostty_render_state_free(render_state: GhosttyRenderStateHandle);
    fn ghostty_render_state_update(
        render_state: GhosttyRenderStateHandle,
        terminal: GhosttyTerminalHandle,
    ) -> i32;
    fn ghostty_render_state_get(
        render_state: GhosttyRenderStateHandle,
        data: u32,
        out: *mut c_void,
    ) -> i32;
    fn ghostty_render_state_row_iterator_new(
        config: *const c_void,
        row_iterator: *mut GhosttyRowIteratorHandle,
    ) -> i32;
    fn ghostty_render_state_row_iterator_free(row_iterator: GhosttyRowIteratorHandle);
    fn ghostty_render_state_row_iterator_next(row_iterator: GhosttyRowIteratorHandle) -> bool;
    fn ghostty_render_state_row_get(
        row_iterator: GhosttyRowIteratorHandle,
        data: u32,
        out: *mut c_void,
    ) -> i32;
    fn ghostty_render_state_row_cells_new(
        config: *const c_void,
        row_cells: *mut GhosttyRowCellsHandle,
    ) -> i32;
    fn ghostty_render_state_row_cells_free(row_cells: GhosttyRowCellsHandle);
    fn ghostty_render_state_row_cells_next(row_cells: GhosttyRowCellsHandle) -> bool;
    fn ghostty_render_state_row_cells_get(
        row_cells: GhosttyRowCellsHandle,
        data: u32,
        out: *mut c_void,
    ) -> i32;
    fn ghostty_cell_get(cell: GhosttyCell, data: u32, out: *mut c_void) -> i32;
    fn ghostty_terminal_get(terminal: GhosttyTerminalHandle, data: u32, out: *mut c_void) -> i32;
}

struct GhosttyApi {
    terminal_new: GhosttyTerminalNew,
    terminal_free: GhosttyTerminalFree,
    terminal_resize: GhosttyTerminalResize,
    terminal_vt_write: GhosttyTerminalVtWrite,
    terminal_scroll_viewport: GhosttyTerminalScrollViewport,
    terminal_get: unsafe extern "C" fn(GhosttyTerminalHandle, u32, *mut c_void) -> i32,
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
        Ok(Self)
    }

    fn load_api(&self) -> Result<Arc<GhosttyApi>, String> {
        GhosttyApi::load()
    }
}

impl TerminalSession {
    pub fn attach(terminal_id: &str, cols: u16, rows: u16) -> Result<Self, String> {
        let runtime = GhosttyRuntime::detect()?;
        let api = runtime.load_api()?;
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

        let (bytes_tx, bytes_rx) = mpsc::channel::<Vec<u8>>();
        let terminal = GhosttyTerminal::new(api, cols, rows)?;

        let bytes_tx_for_reader = bytes_tx.clone();
        #[cfg(unix)]
        {
            let fd = pty
                .master
                .as_raw_fd()
                .ok_or("pty master fd not available")?;
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL, 0) };
            if flags < 0 {
                return Err(format!(
                    "fcntl F_GETFL failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
                return Err(format!(
                    "fcntl F_SETFL failed: {}",
                    std::io::Error::last_os_error()
                ));
            }

            thread::spawn(move || {
                const BATCH_LIMIT: usize = 64 * 1024;
                const MAX_BATCH_MS: u64 = 8;
                let mut buf = [0_u8; 8192];
                let mut accumulated = Vec::new();
                let mut last_send = Instant::now();
                let mut done = false;
                while !done {
                    let mut fds = libc::pollfd {
                        fd,
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    let timeout = if accumulated.is_empty() {
                        -1
                    } else {
                        MAX_BATCH_MS
                            .saturating_sub(last_send.elapsed().as_millis() as u64)
                            .try_into()
                            .unwrap_or(0)
                    };
                    let ret = unsafe { libc::poll(&mut fds, 1, timeout) };
                    if ret < 0 {
                        break;
                    }
                    let readable = ret > 0 && (fds.revents & libc::POLLIN) != 0;
                    let hungup = ret > 0 && (fds.revents & (libc::POLLHUP | libc::POLLERR)) != 0;
                    if readable {
                        loop {
                            let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
                            if n < 0 {
                                let err = std::io::Error::last_os_error();
                                if err.raw_os_error() == Some(libc::EAGAIN)
                                    || err.raw_os_error() == Some(libc::EWOULDBLOCK)
                                {
                                    break;
                                }
                                done = true;
                                break;
                            } else if n == 0 {
                                done = true;
                                break;
                            } else {
                                accumulated.extend_from_slice(&buf[..n as usize]);
                                if accumulated.len() >= BATCH_LIMIT {
                                    break;
                                }
                            }
                        }
                    }
                    if hungup {
                        done = true;
                    }
                    // Forward bytes once the kernel buffer is drained, the PTY
                    // closes, or the 8ms batch window expires. The terminal
                    // lives on the main thread; this thread only handles I/O.
                    if !accumulated.is_empty()
                        && (ret == 0
                            || done
                            || accumulated.len() >= BATCH_LIMIT
                            || last_send.elapsed() >= Duration::from_millis(MAX_BATCH_MS))
                    {
                        let _ = bytes_tx_for_reader.send(accumulated.clone());
                        accumulated.clear();
                        last_send = Instant::now();
                    }
                }
                // Exit immediately when the PTY closes so the main thread sees
                // the disconnect and can switch to the previous terminal. The
                // child cleanup is deferred to a separate thread so this one
                // never blocks on a slow shell exit.
                thread::spawn(move || {
                    let _ = child.kill();
                    let _ = child.wait();
                });
            });
        }
        #[cfg(not(unix))]
        {
            let mut reader = pty
                .master
                .try_clone_reader()
                .map_err(|err| err.to_string())?;
            thread::spawn(move || {
                let mut buf = [0_u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let _ = bytes_tx_for_reader.send(buf[..n].to_vec());
                        }
                        Err(_) => break,
                    }
                }
                let _ = child.kill();
                let _ = child.wait();
            });
        }

        Ok(Self {
            output: Some(bytes_rx),
            terminal,
            master: pty.master,
            killer,
        })
    }

    pub fn resize(
        &mut self,
        cols: u16,
        rows: u16,
        pixel_width: u16,
        pixel_height: u16,
    ) -> Result<TerminalFrame, String> {
        let size = PtySize {
            rows,
            cols,
            pixel_width,
            pixel_height,
        };
        self.master.resize(size).map_err(|err| err.to_string())?;
        self.terminal.resize(size)?;
        self.terminal.frame()
    }

    pub fn scroll(&mut self, rows: isize) -> Result<TerminalFrame, String> {
        self.terminal.scroll(rows);
        self.terminal.frame()
    }

    /// Begin a cell drag at a viewport column and row; no selection is installed yet.
    #[allow(dead_code)]
    pub fn begin_selection_drag(&mut self, point: (u16, u16)) -> Result<(), String> {
        self.terminal.begin_selection_drag(point)
    }

    /// Extend the active drag to a viewport cell. False means the anchor was lost
    /// or cancelled, including when another screen is active.
    #[allow(dead_code)]
    pub fn update_selection_drag(
        &mut self,
        point: (u16, u16),
        rectangle: bool,
    ) -> Result<bool, String> {
        self.terminal.update_selection_drag(point, rectangle)
    }

    /// End the drag without clearing its installed selection.
    #[allow(dead_code)]
    pub fn end_selection_drag(&mut self) {
        self.terminal.end_selection_drag();
    }

    #[allow(dead_code)]
    pub fn select_viewport(
        &mut self,
        start: (u16, u16),
        end: (u16, u16),
        rectangle: bool,
    ) -> Result<(), String> {
        self.terminal.select_viewport(start, end, rectangle)
    }

    #[allow(dead_code)]
    pub fn select_all(&mut self) -> Result<bool, String> {
        self.terminal.select_all()
    }

    #[allow(dead_code)]
    pub fn clear_selection(&mut self) -> Result<(), String> {
        self.terminal.clear_selection()
    }

    #[allow(dead_code)]
    pub fn copy_selection(&self) -> Result<Option<String>, String> {
        self.terminal.copy_selection()
    }

    #[allow(dead_code)]
    pub fn selection_rows(&mut self) -> Result<Vec<Option<(u16, u16)>>, String> {
        self.terminal.selection_rows()
    }

    /// Feed VT bytes without extracting a paint frame (cheap).
    pub fn write_bytes(&mut self, data: &[u8]) {
        self.terminal.write(data);
    }

    /// Full cell-grid extract for paint. Expensive — throttle callers.
    pub fn frame(&mut self) -> Result<TerminalFrame, String> {
        self.terminal.frame()
    }

    #[allow(dead_code)]
    pub fn write(&mut self, data: &[u8]) -> Result<TerminalFrame, String> {
        self.write_bytes(data);
        self.frame()
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
            unsafe { (api.terminal_free)(terminal) };
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
            drag_anchor: None,
        })
    }

    pub fn write(&mut self, bytes: &[u8]) {
        unsafe {
            (self.api.terminal_vt_write)(self.terminal, bytes.as_ptr(), bytes.len());
        }
    }

    pub fn begin_selection_drag(&mut self, point: (u16, u16)) -> Result<(), String> {
        self.end_selection_drag();
        let screen = self.active_screen()?;
        let mut handle = ptr::null_mut();
        let result = unsafe {
            ghostty_terminal_grid_ref_track(
                self.terminal,
                Point::viewport(point.0, point.1),
                &mut handle,
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_terminal_grid_ref_track failed: {result}"));
        }
        self.drag_anchor = Some(DragAnchor { handle, screen });
        Ok(())
    }

    pub fn update_selection_drag(
        &mut self,
        point: (u16, u16),
        rectangle: bool,
    ) -> Result<bool, String> {
        let Some(anchor) = self.drag_anchor.as_ref() else {
            return Ok(false);
        };
        let screen = anchor.screen;
        let handle = anchor.handle;
        if self.active_screen()? != screen {
            self.end_selection_drag();
            return Ok(false);
        }
        let mut selection = Selection::new();
        let result = unsafe { ghostty_tracked_grid_ref_snapshot(handle, &mut selection.start) };
        if result == GHOSTTY_NO_VALUE {
            self.end_selection_drag();
            return Ok(false);
        }
        if result != GHOSTTY_SUCCESS {
            return Err(format!(
                "ghostty_tracked_grid_ref_snapshot failed: {result}"
            ));
        }
        let result = unsafe {
            ghostty_terminal_grid_ref(
                self.terminal,
                Point::viewport(point.0, point.1),
                &mut selection.end,
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_terminal_grid_ref failed: {result}"));
        }
        selection.rectangle = rectangle;
        self.install_selection(Some(&selection))?;
        Ok(true)
    }

    pub fn end_selection_drag(&mut self) {
        self.drag_anchor = None;
    }

    fn active_screen(&self) -> Result<u32, String> {
        let mut screen = 0_u32;
        let result = unsafe {
            (self.api.terminal_get)(
                self.terminal,
                GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN,
                (&mut screen as *mut u32).cast(),
            )
        };
        if result == GHOSTTY_SUCCESS {
            Ok(screen)
        } else {
            Err(format!("ghostty active screen failed: {result}"))
        }
    }

    pub fn select_viewport(
        &mut self,
        start: (u16, u16),
        end: (u16, u16),
        rectangle: bool,
    ) -> Result<(), String> {
        let mut selection = Selection::new();
        for (point, out) in [(start, &mut selection.start), (end, &mut selection.end)] {
            let result = unsafe {
                ghostty_terminal_grid_ref(self.terminal, Point::viewport(point.0, point.1), out)
            };
            if result != GHOSTTY_SUCCESS {
                return Err(format!("ghostty_terminal_grid_ref failed: {result}"));
            }
        }
        selection.rectangle = rectangle;
        self.install_selection(Some(&selection))
    }

    pub fn select_all(&mut self) -> Result<bool, String> {
        let mut selection = Selection::new();
        let result = unsafe { ghostty_terminal_select_all(self.terminal, &mut selection) };
        if result == GHOSTTY_NO_VALUE {
            return Ok(false);
        }
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_terminal_select_all failed: {result}"));
        }
        self.install_selection(Some(&selection))?;
        Ok(true)
    }

    pub fn clear_selection(&mut self) -> Result<(), String> {
        self.end_selection_drag();
        self.install_selection(None)
    }

    fn install_selection(&mut self, selection: Option<&Selection>) -> Result<(), String> {
        // Ghostty copies the snapshot into terminal-owned tracked endpoints during this call.
        let value = selection.map_or(ptr::null(), |s| (s as *const Selection).cast());
        let result = unsafe { ghostty_terminal_set(self.terminal, 21, value) };
        if result == GHOSTTY_SUCCESS {
            Ok(())
        } else {
            Err(format!("ghostty_terminal_set selection failed: {result}"))
        }
    }

    pub fn copy_selection(&self) -> Result<Option<String>, String> {
        let options = || SelectionFormatOptions {
            size: std::mem::size_of::<SelectionFormatOptions>(),
            emit: 0,
            unwrap: true,
            trim: true,
            selection: ptr::null(),
        };
        let mut len = 0;
        let result = unsafe {
            ghostty_terminal_selection_format_buf(
                self.terminal,
                options(),
                ptr::null_mut(),
                0,
                &mut len,
            )
        };
        if result == GHOSTTY_NO_VALUE {
            return Ok(None);
        }
        if result != -3 && result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty selection size failed: {result}"));
        }
        let mut bytes = vec![0; len];
        let result = unsafe {
            ghostty_terminal_selection_format_buf(
                self.terminal,
                options(),
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut len,
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty selection format failed: {result}"));
        }
        bytes.truncate(len);
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|err| err.to_string())
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

    fn default_bg(&self) -> Result<Option<u32>, String> {
        let mut color = GhosttyColorRgb::default();
        let result = unsafe {
            (self.api.terminal_get)(
                self.terminal,
                GHOSTTY_TERMINAL_DATA_COLOR_BACKGROUND_DEFAULT,
                (&mut color as *mut GhosttyColorRgb).cast(),
            )
        };
        if result == GHOSTTY_NO_VALUE {
            return Ok(None);
        }
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_terminal_get default bg failed: {result}"));
        }
        Ok(Some(rgb_u32(color.r, color.g, color.b)))
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
        let default_bg = self.default_bg()?;
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
            let mut line = TerminalLine {
                selection: self.row_selection()?,
                ..TerminalLine::default()
            };
            let mut x = 0_u16;
            while unsafe { (self.api.row_cells_next)(self.row_cells) } {
                let (kind, text) = self.cell_text()?;
                let cursor_here = cursor.is_some_and(|cursor| cursor == (x, y));
                let (fg, bg) = if cursor_here {
                    (0x0a0a0a, Some(0xf4f4f4))
                } else {
                    (
                        self.cell_color(ROW_CELLS_DATA_FG_COLOR)?
                            .unwrap_or(DEFAULT_FG),
                        terminal_bg(self.cell_color(ROW_CELLS_DATA_BG_COLOR)?, default_bg),
                    )
                };
                line.cells.push(TerminalCell {
                    column: x,
                    span: if kind == TerminalCellKind::Wide { 2 } else { 1 },
                    kind,
                    text: text.clone(),
                    fg,
                    bg,
                });
                x = x.saturating_add(1);
                if !text.is_empty() {
                    push_run(&mut line.runs, text, fg, bg);
                }
            }
            lines.push(line);
            y = y.saturating_add(1);
        }
        Ok(TerminalFrame { lines })
    }

    pub fn selection_rows(&mut self) -> Result<Vec<Option<(u16, u16)>>, String> {
        let result = unsafe { (self.api.render_state_update)(self.render_state, self.terminal) };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty render update failed: {result}"));
        }
        let result = unsafe {
            (self.api.render_state_get)(
                self.render_state,
                RENDER_STATE_DATA_ROW_ITERATOR,
                (&mut self.row_iterator as *mut GhosttyRowIteratorHandle).cast(),
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty row iterator failed: {result}"));
        }
        let mut rows = Vec::new();
        while unsafe { (self.api.row_iterator_next)(self.row_iterator) } {
            rows.push(self.row_selection()?);
        }
        Ok(rows)
    }

    fn row_selection(&self) -> Result<Option<(u16, u16)>, String> {
        let mut selection = RowSelection {
            size: std::mem::size_of::<RowSelection>(),
            start_x: 0,
            end_x: 0,
        };
        let result = unsafe {
            (self.api.row_get)(
                self.row_iterator,
                RENDER_STATE_ROW_DATA_SELECTION,
                (&mut selection as *mut RowSelection).cast(),
            )
        };
        match result {
            GHOSTTY_SUCCESS => Ok(Some((selection.start_x, selection.end_x))),
            GHOSTTY_NO_VALUE => Ok(None),
            _ => Err(format!("ghostty row selection failed: {result}")),
        }
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

    fn cell_text(&self) -> Result<(TerminalCellKind, String), String> {
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
        let kind = match wide {
            CELL_WIDE_NARROW => TerminalCellKind::Narrow,
            CELL_WIDE_WIDE => TerminalCellKind::Wide,
            CELL_WIDE_SPACER_TAIL => TerminalCellKind::SpacerTail,
            CELL_WIDE_SPACER_HEAD => TerminalCellKind::SpacerHead,
            _ => return Err(format!("unknown ghostty cell width: {wide}")),
        };
        if matches!(
            kind,
            TerminalCellKind::SpacerTail | TerminalCellKind::SpacerHead
        ) {
            return Ok((kind, String::new()));
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
            return Ok((
                kind,
                codepoints
                    .into_iter()
                    .map(|codepoint| {
                        char::from_u32(codepoint).unwrap_or(char::REPLACEMENT_CHARACTER)
                    })
                    .collect(),
            ));
        }

        let mut has_text = false;
        let result = unsafe {
            (self.api.cell_get)(raw, CELL_DATA_HAS_TEXT, (&mut has_text as *mut bool).cast())
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty cell has text failed: {result}"));
        }
        if !has_text {
            return Ok((kind, " ".to_string()));
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
        Ok((
            kind,
            char::from_u32(codepoint)
                .unwrap_or(char::REPLACEMENT_CHARACTER)
                .to_string(),
        ))
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
            GHOSTTY_SUCCESS => Ok(Some(rgb_u32(color.r, color.g, color.b))),
            GHOSTTY_INVALID_VALUE => Ok(None),
            other => Err(format!("ghostty row cell color failed: {other}")),
        }
    }
}

impl Drop for GhosttyTerminal {
    fn drop(&mut self) {
        self.end_selection_drag();
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
    fn load() -> Result<Arc<Self>, String> {
        Ok(Arc::new(Self {
            terminal_new: ghostty_terminal_new,
            terminal_free: ghostty_terminal_free,
            terminal_resize: ghostty_terminal_resize,
            terminal_vt_write: ghostty_terminal_vt_write,
            terminal_scroll_viewport: ghostty_terminal_scroll_viewport,
            terminal_get: ghostty_terminal_get,
            render_state_new: ghostty_render_state_new,
            render_state_free: ghostty_render_state_free,
            render_state_update: ghostty_render_state_update,
            render_state_get: ghostty_render_state_get,
            row_iterator_new: ghostty_render_state_row_iterator_new,
            row_iterator_free: ghostty_render_state_row_iterator_free,
            row_iterator_next: ghostty_render_state_row_iterator_next,
            row_get: ghostty_render_state_row_get,
            row_cells_new: ghostty_render_state_row_cells_new,
            row_cells_free: ghostty_render_state_row_cells_free,
            row_cells_next: ghostty_render_state_row_cells_next,
            row_cells_get: ghostty_render_state_row_cells_get,
            cell_get: ghostty_cell_get,
        }))
    }
}

impl TerminalFrame {
    pub fn from_ansi(cols: u16, rows: u16, ansi: &str) -> Result<Self, String> {
        let runtime = GhosttyRuntime::detect()?;
        let api = runtime.load_api()?;
        let mut terminal = GhosttyTerminal::new(api, cols, rows)?;
        terminal.write(ansi.as_bytes());
        terminal.frame()
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

fn rgb_u32(r: u8, g: u8, b: u8) -> u32 {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

fn terminal_bg(color: Option<u32>, default_bg: Option<u32>) -> Option<u32> {
    let color = color?;
    if default_bg == Some(color) {
        return None;
    }
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

#[cfg(test)]
mod tests {
    use super::{
        push_run, terminal_bg, GhosttyRuntime, GhosttyTerminal, TerminalFrame, TerminalLine,
    };

    #[cfg(target_os = "macos")]
    #[test]
    fn drag_anchor_tracks_viewport_scroll() -> Result<(), String> {
        let mut terminal = GhosttyTerminal::new(GhosttyRuntime::detect()?.load_api()?, 8, 3)?;
        terminal.write(b"one\r\ntwo\r\nthree\r\nfour");
        terminal.scroll(-1);
        terminal.begin_selection_drag((0, 0))?;
        terminal.write(b"");
        terminal.scroll(1);
        assert!(terminal.update_selection_drag((2, 0), false)?);
        assert_eq!(terminal.copy_selection()?.as_deref(), Some("one\ntwo"));
        terminal.end_selection_drag();
        assert!(!terminal.update_selection_drag((0, 0), false)?);
        assert_eq!(terminal.copy_selection()?.as_deref(), Some("one\ntwo"));
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn drag_survives_output_and_screen_roundtrip() -> Result<(), String> {
        let mut terminal = GhosttyTerminal::new(GhosttyRuntime::detect()?.load_api()?, 8, 3)?;
        terminal.write(b"primary");
        terminal.begin_selection_drag((0, 0))?;
        terminal.write(b"x");
        assert!(terminal.update_selection_drag((6, 0), false)?);
        assert_eq!(terminal.copy_selection()?.as_deref(), Some("primary"));
        terminal.write(b"\x1b[?1049hother\x1b[?1049l");
        assert!(terminal.update_selection_drag((6, 0), false)?);
        assert_eq!(terminal.copy_selection()?.as_deref(), Some("primary"));
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn drag_rejects_anchor_after_screen_reset() -> Result<(), String> {
        let mut terminal = GhosttyTerminal::new(GhosttyRuntime::detect()?.load_api()?, 8, 3)?;
        terminal.write(b"\x1b[?1049halternate");
        terminal.begin_selection_drag((0, 0))?;
        terminal.write(b"\x1bc\x1b[?1049h");
        assert!(!terminal.update_selection_drag((1, 0), false)?);
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn drag_cancels_when_screen_changes() -> Result<(), String> {
        let mut terminal = GhosttyTerminal::new(GhosttyRuntime::detect()?.load_api()?, 8, 3)?;
        terminal.write(b"primary");
        terminal.begin_selection_drag((0, 0))?;
        terminal.write(b"\x1b[?1049h");
        assert!(!terminal.update_selection_drag((0, 0), false)?);
        terminal.write(b"\x1b[?1049l");
        assert!(!terminal.update_selection_drag((0, 0), false)?);
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn selection_copy_render_and_clear() -> Result<(), String> {
        let mut terminal = GhosttyTerminal::new(GhosttyRuntime::detect()?.load_api()?, 12, 3)?;
        terminal.write(b"hello world\r\nsecond");
        terminal.select_viewport((4, 0), (0, 0), false)?;
        assert_eq!(terminal.copy_selection()?.as_deref(), Some("hello"));
        assert_eq!(terminal.selection_rows()?[0], Some((0, 4)));
        assert_eq!(terminal.frame()?.lines[0].selection, Some((0, 4)));
        terminal.write(b"!");
        assert_eq!(terminal.copy_selection()?.as_deref(), Some("hello"));
        terminal.clear_selection()?;
        assert_eq!(terminal.copy_selection()?, None);
        assert!(terminal.selection_rows()?.iter().all(Option::is_none));
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn frame_preserves_cell_columns_and_wide_spacers() -> Result<(), String> {
        use super::TerminalCellKind;

        let mut terminal = GhosttyTerminal::new(GhosttyRuntime::detect()?.load_api()?, 8, 2)?;
        terminal.write("a 界b".as_bytes());
        terminal.select_viewport((1, 0), (3, 0), false)?;
        let frame = terminal.frame()?;
        let line = &frame.lines[0];
        assert_eq!(line.selection, Some((1, 3)));
        assert_eq!(line.cells.len(), 8);
        assert_eq!(line.cells[0].text, "a");
        assert_eq!(line.cells[1].text, " ");
        assert_eq!(line.cells[2].column, 2);
        assert_eq!(line.cells[2].text, "界");
        assert_eq!(line.cells[2].kind, TerminalCellKind::Wide);
        assert_eq!(line.cells[2].span, 2);
        assert_eq!(line.cells[3].column, 3);
        assert_eq!(line.cells[3].kind, TerminalCellKind::SpacerTail);
        assert!(line.cells[3].text.is_empty());
        assert_eq!(line.cells[4].column, 4);
        assert_eq!(line.cells[4].text, "b");
        assert_eq!(line.cells[7].text, " ");
        Ok(())
    }

    #[test]
    fn detect_should_find_local_ghostty_checkout() {
        let runtime = GhosttyRuntime::detect();
        assert!(runtime.is_ok() || runtime.is_err());
    }

    #[test]
    fn terminal_runs_merge_adjacent_same_style() {
        let mut line = TerminalLine::default();
        push_run(&mut line.runs, "a".to_string(), 0x111111, None);
        push_run(&mut line.runs, "b".to_string(), 0x111111, None);
        push_run(&mut line.runs, "c".to_string(), 0x222222, None);
        assert_eq!(line.runs.len(), 2);
        assert_eq!(line.runs[0].text, "ab");
        assert_eq!(line.runs[1].text, "c");
    }

    #[test]
    fn terminal_bg_drops_default_background() {
        assert_eq!(terminal_bg(Some(0x282c34), Some(0x282c34)), None);
        assert_eq!(terminal_bg(Some(0x5f1f2a), Some(0x282c34)), Some(0x5f1f2a));
        assert_eq!(terminal_bg(Some(0x00aa00), Some(0x1e1e1e)), Some(0x00aa00));
    }

    #[test]
    fn terminal_bg_drops_neutral_default_backgrounds() {
        assert_eq!(terminal_bg(Some(0x282c34), None), None);
        assert_eq!(terminal_bg(Some(0x303743), None), None);
        assert_eq!(terminal_bg(Some(0x5f1f2a), None), Some(0x5f1f2a));
        assert_eq!(terminal_bg(Some(0x00aa00), None), Some(0x00aa00));
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
