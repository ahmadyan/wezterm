//! C FFI wrapper around wezterm-term for embedding in native applications.
//!
//! This library exposes a C-compatible API for creating and managing terminal
//! emulator instances backed by WezTerm's terminal engine. It is designed to
//! be embedded in macOS applications (via xcframework) where the host app
//! provides PTY management and rendering, while this library handles:
//!
//! - VT escape sequence parsing and terminal state management
//! - Keyboard/mouse input encoding to escape sequences
//! - Screen state queries (lines, cells, cursor, colors)
//! - Title and working directory change notifications via callbacks

mod config;
mod embed;
mod font_config;
mod render;
mod surface;
mod types;

use crate::config::FFITerminalConfig;
use crate::types::*;
use std::ffi::{CStr, CString};
use std::io::Write;
use std::os::raw::c_char;
use std::ptr;
use std::slice;
use std::sync::{Arc, Mutex};
use wezterm_term::terminal::{Alert, Terminal};
use wezterm_term::TerminalSize;

// ---------------------------------------------------------------------------
// Internal writer that captures bytes the terminal wants to send to the PTY.
// The host application reads these bytes via wezterm_take_output() and writes
// them to its own PTY file descriptor.
// ---------------------------------------------------------------------------

struct OutputCapture {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl Write for OutputCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Internal alert handler that captures title/cwd/bell notifications
// and forwards them to C callbacks.
// ---------------------------------------------------------------------------

struct FFIAlertHandler {
    callbacks: WezTermCallbacks,
    context: *mut libc::c_void,
}

// SAFETY: The context pointer is managed by the host application which
// ensures it remains valid for the lifetime of the terminal.
unsafe impl Send for FFIAlertHandler {}
unsafe impl Sync for FFIAlertHandler {}

impl wezterm_term::terminal::AlertHandler for FFIAlertHandler {
    fn alert(&mut self, alert: Alert) {
        match alert {
            Alert::Bell => {
                if let Some(cb) = self.callbacks.on_bell {
                    unsafe { cb(self.context) };
                }
            }
            Alert::WindowTitleChanged(title) => {
                if let Some(cb) = self.callbacks.on_title_changed {
                    if let Ok(c_title) = CString::new(title) {
                        unsafe { cb(self.context, c_title.as_ptr()) };
                    }
                }
            }
            Alert::CurrentWorkingDirectoryChanged => {
                // CWD is queried separately via wezterm_get_current_dir()
                if let Some(cb) = self.callbacks.on_cwd_changed {
                    unsafe { cb(self.context) };
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Selection tracking for copy/paste support.
// ---------------------------------------------------------------------------

/// Represents a point in terminal coordinates (column, row)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SelectionPoint {
    x: usize,
    y: isize,
}

/// Tracks the current text selection state
#[derive(Debug, Clone)]
struct SelectionRange {
    start: SelectionPoint,
    end: SelectionPoint,
}

impl SelectionRange {
    /// Returns (start, end) in normalized order (start <= end)
    fn normalized(&self) -> (SelectionPoint, SelectionPoint) {
        if self.start.y < self.end.y || (self.start.y == self.end.y && self.start.x <= self.end.x) {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }
}

// ---------------------------------------------------------------------------
// Opaque handle wrapping the terminal instance and associated state.
// ---------------------------------------------------------------------------

pub(crate) struct TerminalInstance {
    pub(crate) terminal: Terminal,
    pub(crate) output_buffer: Arc<Mutex<Vec<u8>>>,
    /// Viewport offset for scrollback viewing. 0 means showing the live terminal output,
    /// positive values scroll back into history.
    pub(crate) viewport_offset: isize,
    /// Current text selection, if any
    selection: Option<SelectionRange>,
    /// Whether a mouse drag selection is in progress
    selecting: bool,
}

/// Opaque handle to a WezTerm terminal instance.
/// Created by `wezterm_new()`, freed by `wezterm_free()`.
pub struct WezTermHandle {
    pub(crate) inner: Box<TerminalInstance>,
}

// ===================================================================
// FFI API
// ===================================================================

/// Initialize the library. Call once before any other function.
/// Sets up logging if WEZTERM_LOG env var is set.
#[no_mangle]
pub extern "C" fn wezterm_init() {
    let _ = env_logger::try_init();
}

/// Create a new terminal instance.
///
/// # Parameters
/// - `rows`, `cols`: visible terminal dimensions in character cells
/// - `pixel_width`, `pixel_height`: pixel dimensions (used for image protocols)
/// - `config`: optional configuration (NULL for defaults)
/// - `callbacks`: event callbacks (title change, bell, etc.)
/// - `callback_context`: opaque pointer passed to all callbacks
///
/// # Returns
/// Opaque handle, or NULL on failure. Must be freed with `wezterm_free()`.
#[no_mangle]
pub extern "C" fn wezterm_new(
    rows: u32,
    cols: u32,
    pixel_width: u32,
    pixel_height: u32,
    config: *const WezTermConfig,
    callbacks: WezTermCallbacks,
    callback_context: *mut libc::c_void,
) -> *mut WezTermHandle {
    let size = TerminalSize {
        rows: rows as usize,
        cols: cols as usize,
        pixel_width: pixel_width as usize,
        pixel_height: pixel_height as usize,
        dpi: 0,
    };

    let term_config: Arc<dyn wezterm_term::config::TerminalConfiguration + Send + Sync> =
        if config.is_null() {
            Arc::new(FFITerminalConfig::default())
        } else {
            Arc::new(unsafe { FFITerminalConfig::from_ffi(&*config) })
        };

    let output_buffer = Arc::new(Mutex::new(Vec::with_capacity(4096)));
    let writer = Box::new(OutputCapture {
        buffer: Arc::clone(&output_buffer),
    });

    let mut terminal = Terminal::new(
        size,
        term_config,
        "Agentastic",
        env!("CARGO_PKG_VERSION"),
        writer,
    );

    // Set up alert handler for title/bell/cwd notifications
    let alert_handler = FFIAlertHandler {
        callbacks,
        context: callback_context,
    };
    terminal.set_notification_handler(Box::new(alert_handler));

    let handle = Box::new(WezTermHandle {
        inner: Box::new(TerminalInstance {
            terminal,
            output_buffer,
            viewport_offset: 0,
            selection: None,
            selecting: false,
        }),
    });

    Box::into_raw(handle)
}

/// Free a terminal instance and all associated resources.
///
/// # Safety
/// `handle` must be a valid pointer returned by `wezterm_new()`, or NULL.
/// After this call, the handle is invalid and must not be used.
#[no_mangle]
pub unsafe extern "C" fn wezterm_free(handle: *mut WezTermHandle) {
    if !handle.is_null() {
        drop(Box::from_raw(handle));
    }
}

/// Feed bytes from the PTY output into the terminal emulator.
/// This is the primary way to update the terminal state — call this
/// whenever the PTY produces output.
///
/// # Safety
/// `handle` must be valid. `data` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn wezterm_advance_bytes(
    handle: *mut WezTermHandle,
    data: *const u8,
    len: usize,
) {
    if handle.is_null() || data.is_null() {
        return;
    }
    let term = &mut (*handle).inner.terminal;
    let bytes = slice::from_raw_parts(data, len);
    term.advance_bytes(bytes);
}

/// Take any pending output bytes that the terminal wants to send to the PTY.
/// This includes responses to device queries, keyboard input encoding, etc.
///
/// The caller must free the returned buffer with `wezterm_free_bytes()`.
///
/// # Returns
/// Number of bytes written to `*out_data`. Sets `*out_data` to a newly
/// allocated buffer, or NULL if there is no pending output.
///
/// # Safety
/// `handle` and `out_data` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn wezterm_take_output(
    handle: *mut WezTermHandle,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) {
    if handle.is_null() || out_data.is_null() || out_len.is_null() {
        return;
    }

    let buffer = &(*handle).inner.output_buffer;
    let mut buf = buffer.lock().unwrap();

    if buf.is_empty() {
        *out_data = ptr::null_mut();
        *out_len = 0;
        return;
    }

    let data = std::mem::take(&mut *buf);
    let len = data.len();
    let ptr = libc::malloc(len) as *mut u8;
    if ptr.is_null() {
        *out_data = ptr::null_mut();
        *out_len = 0;
        return;
    }
    ptr::copy_nonoverlapping(data.as_ptr(), ptr, len);
    *out_data = ptr;
    *out_len = len;
}

/// Free a byte buffer returned by `wezterm_take_output()`.
///
/// # Safety
/// `data` must be a pointer returned by `wezterm_take_output()`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn wezterm_free_bytes(data: *mut u8) {
    if !data.is_null() {
        libc::free(data as *mut libc::c_void);
    }
}

/// Resize the terminal.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_resize(
    handle: *mut WezTermHandle,
    rows: u32,
    cols: u32,
    pixel_width: u32,
    pixel_height: u32,
) {
    if handle.is_null() {
        return;
    }
    let term = &mut (*handle).inner.terminal;
    let size = TerminalSize {
        rows: rows as usize,
        cols: cols as usize,
        pixel_width: pixel_width as usize,
        pixel_height: pixel_height as usize,
        dpi: 0,
    };
    term.resize(size);
}

/// Get the current cursor position and shape.
///
/// # Safety
/// `handle` and `out` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn wezterm_get_cursor(
    handle: *const WezTermHandle,
    out: *mut WezTermCursorInfo,
) {
    if handle.is_null() || out.is_null() {
        return;
    }
    let term = &(*handle).inner.terminal;
    let pos = term.cursor_pos();

    *out = WezTermCursorInfo {
        x: pos.x as u32,
        y: pos.y as i32,
        shape: WezTermCursorShape::from(pos.shape),
        visible: matches!(
            pos.visibility,
            wezterm_surface::CursorVisibility::Visible
        ),
    };
}

/// Get the number of visible rows in the terminal.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_get_visible_rows(handle: *const WezTermHandle) -> u32 {
    if handle.is_null() {
        return 0;
    }
    (*handle).inner.terminal.screen().physical_rows as u32
}

/// Get the number of visible columns in the terminal.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_get_visible_cols(handle: *const WezTermHandle) -> u32 {
    if handle.is_null() {
        return 0;
    }
    (*handle).inner.terminal.screen().physical_cols as u32
}

/// Get the total number of lines (scrollback + visible).
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_get_total_rows(handle: *const WezTermHandle) -> u32 {
    if handle.is_null() {
        return 0;
    }
    (*handle).inner.terminal.screen().scrollback_rows() as u32
}

/// Read the cells of a visible row into a caller-provided buffer.
///
/// # Parameters
/// - `row`: visible row index (0 = top of visible area)
/// - `out_cells`: caller-allocated buffer for cell data
/// - `max_cells`: capacity of `out_cells`
///
/// # Returns
/// Number of cells actually written (clamped to `max_cells` and row width).
///
/// # Safety
/// `handle` must be valid. `out_cells` must have room for `max_cells` items.
#[no_mangle]
pub unsafe extern "C" fn wezterm_get_row_cells(
    handle: *mut WezTermHandle,
    row: i32,
    out_cells: *mut WezTermCellInfo,
    max_cells: u32,
) -> u32 {
    if handle.is_null() || out_cells.is_null() || max_cells == 0 {
        return 0;
    }

    let term = &mut (*handle).inner.terminal;

    // Get palette first (immutable borrow), then access screen (mutable borrow)
    let palette = term.palette();
    let visible_row = row as wezterm_term::VisibleRowIndex;
    let screen = term.screen_mut();
    let phys = screen.phys_row(visible_row);
    let cols = screen.physical_cols;
    let line = screen.line_mut(phys);
    let cols = cols.min(max_cells as usize);

    let mut count = 0u32;
    for cell_ref in line.visible_cells() {
        let idx = cell_ref.cell_index();
        if idx >= cols {
            break;
        }

        let attrs = cell_ref.attrs();
        let fg_color = palette.resolve_fg(attrs.foreground());
        let bg_color = palette.resolve_bg(attrs.background());
        let text = cell_ref.str();

        let text_bytes = text.as_bytes();
        let mut text_buf = [0u8; 8];
        let copy_len = text_bytes.len().min(7);
        text_buf[..copy_len].copy_from_slice(&text_bytes[..copy_len]);

        let out = &mut *out_cells.add(idx);
        *out = WezTermCellInfo {
            text: text_buf,
            text_len: copy_len as u8,
            width: cell_ref.width() as u8,
            fg: WezTermColorRGBA::from_srgba(fg_color),
            bg: WezTermColorRGBA::from_srgba(bg_color),
            bold: matches!(attrs.intensity(), wezterm_cell::Intensity::Bold),
            italic: attrs.italic(),
            underline: WezTermUnderline::from(attrs.underline()),
            strikethrough: attrs.strikethrough(),
            reverse: attrs.reverse(),
            invisible: attrs.invisible(),
        };

        count = (idx as u32) + 1;
    }

    // Fill remaining cells with blanks
    for i in count..cols as u32 {
        let out = &mut *out_cells.add(i as usize);
        *out = WezTermCellInfo::blank();
    }

    cols as u32
}

/// Read the cells of a scrollback row (by stable index) into a caller buffer.
///
/// # Parameters
/// - `stable_row`: stable row index (0 = first line in scrollback)
/// - `out_cells`: caller-allocated buffer for cell data
/// - `max_cells`: capacity of `out_cells`
///
/// # Returns
/// Number of cells actually written, or 0 if the row is out of range.
///
/// # Safety
/// `handle` must be valid. `out_cells` must have room for `max_cells` items.
#[no_mangle]
pub unsafe extern "C" fn wezterm_get_scrollback_row_cells(
    handle: *mut WezTermHandle,
    stable_row: isize,
    out_cells: *mut WezTermCellInfo,
    max_cells: u32,
) -> u32 {
    if handle.is_null() || out_cells.is_null() || max_cells == 0 {
        return 0;
    }

    let term = &mut (*handle).inner.terminal;

    // Check bounds with immutable borrow first
    let total = term.screen().scrollback_rows();
    if stable_row < 0 || (stable_row as usize) >= total {
        return 0;
    }

    let palette = term.palette();
    let screen = term.screen_mut();
    let cols = screen.physical_cols;
    let line = screen.line_mut(stable_row as usize);
    let cols = cols.min(max_cells as usize);

    let mut count = 0u32;
    for cell_ref in line.visible_cells() {
        let idx = cell_ref.cell_index();
        if idx >= cols {
            break;
        }

        let attrs = cell_ref.attrs();
        let fg_color = palette.resolve_fg(attrs.foreground());
        let bg_color = palette.resolve_bg(attrs.background());
        let text = cell_ref.str();

        let text_bytes = text.as_bytes();
        let mut text_buf = [0u8; 8];
        let copy_len = text_bytes.len().min(7);
        text_buf[..copy_len].copy_from_slice(&text_bytes[..copy_len]);

        let out = &mut *out_cells.add(idx);
        *out = WezTermCellInfo {
            text: text_buf,
            text_len: copy_len as u8,
            width: cell_ref.width() as u8,
            fg: WezTermColorRGBA::from_srgba(fg_color),
            bg: WezTermColorRGBA::from_srgba(bg_color),
            bold: matches!(attrs.intensity(), wezterm_cell::Intensity::Bold),
            italic: attrs.italic(),
            underline: WezTermUnderline::from(attrs.underline()),
            strikethrough: attrs.strikethrough(),
            reverse: attrs.reverse(),
            invisible: attrs.invisible(),
        };

        count = (idx as u32) + 1;
    }

    for i in count..cols as u32 {
        let out = &mut *out_cells.add(i as usize);
        *out = WezTermCellInfo::blank();
    }

    cols as u32
}

/// Send a key-down event to the terminal. The terminal encodes this
/// into the appropriate escape sequence and buffers it for output.
/// Use `wezterm_take_output()` to retrieve the encoded bytes.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_key_down(
    handle: *mut WezTermHandle,
    key: WezTermKeyCode,
    modifiers: u32,
) -> bool {
    if handle.is_null() {
        return false;
    }
    let term = &mut (*handle).inner.terminal;
    let keycode = key.to_termwiz();
    let mods = termwiz::input::Modifiers::from_bits_truncate(modifiers as u16);

    term.key_down(keycode, mods).is_ok()
}

/// Send a key-down event for a printable character. Unlike `wezterm_key_down`,
/// this preserves the original codepoint so the terminal can encode modifier
/// combinations correctly instead of forcing the caller down the raw-byte path.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_key_down_char(
    handle: *mut WezTermHandle,
    ch: u32,
    modifiers: u32,
) -> bool {
    if handle.is_null() {
        return false;
    }

    let Some(ch) = char::from_u32(ch) else {
        return false;
    };

    let term = &mut (*handle).inner.terminal;
    let mods = termwiz::input::Modifiers::from_bits_truncate(modifiers as u16);
    term.key_down(termwiz::input::KeyCode::Char(ch), mods).is_ok()
}

/// Send a key-up event to the terminal (used by Kitty keyboard protocol).
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_key_up(
    handle: *mut WezTermHandle,
    key: WezTermKeyCode,
    modifiers: u32,
) -> bool {
    if handle.is_null() {
        return false;
    }
    let term = &mut (*handle).inner.terminal;
    let keycode = key.to_termwiz();
    let mods = termwiz::input::Modifiers::from_bits_truncate(modifiers as u16);

    term.key_up(keycode, mods).is_ok()
}

/// Send a mouse event to the terminal.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_mouse_event(
    handle: *mut WezTermHandle,
    kind: WezTermMouseEventKind,
    x: u32,
    y: i32,
    button: WezTermMouseButton,
    modifiers: u32,
) -> bool {
    if handle.is_null() {
        return false;
    }
    let instance = &mut (*handle).inner;

    // Handle selection tracking for left mouse button
    let is_left_button = matches!(button, WezTermMouseButton::Left);
    let kind_termwiz = kind.to_termwiz();

    if is_left_button {
        use wezterm_term::input::MouseEventKind as MEK;
        match kind_termwiz {
            MEK::Press => {
                // Start new selection
                let point = SelectionPoint {
                    x: x as usize,
                    y: y as isize - instance.viewport_offset,
                };
                instance.selection = Some(SelectionRange {
                    start: point,
                    end: point,
                });
                instance.selecting = true;
            }
            MEK::Move if instance.selecting => {
                // Extend selection during drag
                if let Some(ref mut sel) = instance.selection {
                    sel.end = SelectionPoint {
                        x: x as usize,
                        y: y as isize - instance.viewport_offset,
                    };
                }
            }
            MEK::Release => {
                // End selection
                instance.selecting = false;
            }
            _ => {}
        }
    }

    let event = wezterm_term::input::MouseEvent {
        kind: kind_termwiz,
        x: x as usize,
        y: y as wezterm_term::VisibleRowIndex,
        x_pixel_offset: 0,
        y_pixel_offset: 0,
        button: button.to_termwiz(),
        modifiers: termwiz::input::Modifiers::from_bits_truncate(modifiers as u16),
    };

    instance.terminal.mouse_event(event).is_ok()
}

/// Send a paste operation to the terminal.
/// Handles bracketed paste mode automatically.
///
/// # Safety
/// `handle` must be valid. `text` must be a valid UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn wezterm_send_paste(
    handle: *mut WezTermHandle,
    text: *const c_char,
) -> bool {
    if handle.is_null() || text.is_null() {
        return false;
    }
    let term = &mut (*handle).inner.terminal;
    let c_str = CStr::from_ptr(text);
    match c_str.to_str() {
        Ok(s) => term.send_paste(s).is_ok(),
        Err(_) => false,
    }
}

/// Send committed UTF-8 text directly to the terminal's writer.
/// Unlike `wezterm_send_paste()`, this does not wrap the text in bracketed-paste
/// sequences; it is intended for normal text input that was already composed by
/// the host input method.
///
/// # Safety
/// `handle` must be valid. `text` must be a valid UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn wezterm_send_text(
    handle: *mut WezTermHandle,
    text: *const c_char,
) -> bool {
    if handle.is_null() || text.is_null() {
        return false;
    }
    let c_str = CStr::from_ptr(text);
    match c_str.to_str() {
        Ok(s) => {
            let buffer = &(*handle).inner.output_buffer;
            buffer.lock().unwrap().extend_from_slice(s.as_bytes());
            true
        }
        Err(_) => false,
    }
}

/// Notify the terminal that focus has changed.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_focus_changed(handle: *mut WezTermHandle, focused: bool) {
    if handle.is_null() {
        return;
    }
    (*handle).inner.terminal.focus_changed(focused);
}

/// Get the current terminal title (set via OSC escape sequences).
///
/// # Returns
/// A newly allocated C string, or NULL if no title is set.
/// The caller must free it with `wezterm_free_string()`.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_get_title(handle: *const WezTermHandle) -> *mut c_char {
    if handle.is_null() {
        return ptr::null_mut();
    }
    let title = (*handle).inner.terminal.get_title();
    match CString::new(title) {
        Ok(c) => c.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

/// Get the current working directory (set via OSC 7).
///
/// # Returns
/// A newly allocated C string, or NULL if no CWD is set.
/// The caller must free it with `wezterm_free_string()`.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_get_current_dir(handle: *const WezTermHandle) -> *mut c_char {
    if handle.is_null() {
        return ptr::null_mut();
    }
    match (*handle).inner.terminal.get_current_dir() {
        Some(url) => match CString::new(url.as_str()) {
            Ok(c) => c.into_raw(),
            Err(_) => ptr::null_mut(),
        },
        None => ptr::null_mut(),
    }
}

/// Free a string returned by `wezterm_get_title()` or `wezterm_get_current_dir()`.
///
/// # Safety
/// `s` must be a pointer returned by one of the string-returning functions, or NULL.
#[no_mangle]
pub unsafe extern "C" fn wezterm_free_string(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}

// ---------------------------------------------------------------------------
// Selection API
// ---------------------------------------------------------------------------

/// Check whether there is an active text selection.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_has_selection(handle: *const WezTermHandle) -> bool {
    if handle.is_null() {
        return false;
    }
    let instance = &(*handle).inner;
    if let Some(ref sel) = instance.selection {
        // Only report selection if start != end
        sel.start != sel.end
    } else {
        false
    }
}

/// Get the currently selected text.
///
/// # Returns
/// A newly allocated C string containing the selected text, or NULL if no selection.
/// The caller must free it with `wezterm_free_string()`.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_get_selection(handle: *mut WezTermHandle) -> *mut c_char {
    if handle.is_null() {
        return ptr::null_mut();
    }
    let instance = &mut (*handle).inner;
    let selection = match &instance.selection {
        Some(sel) if sel.start != sel.end => sel.clone(),
        _ => return ptr::null_mut(),
    };

    let (start, end) = selection.normalized();
    let screen = instance.terminal.screen_mut();
    let cols = screen.physical_cols;
    let mut result = String::new();

    // Iterate through selected rows
    for row in start.y..=end.y {
        // Get the line - convert to physical row index
        let visible_row = row as wezterm_term::VisibleRowIndex;
        let phys = screen.phys_row(visible_row);

        // Determine column range for this row
        let start_col = if row == start.y { start.x } else { 0 };
        let end_col = if row == end.y { end.x + 1 } else { cols };

        // Read cells from the line
        let line = screen.line_mut(phys);
        for cell in line.visible_cells() {
            let idx = cell.cell_index();
            if idx >= start_col && idx < end_col {
                result.push_str(cell.str());
            }
        }

        // Add newline between rows (but not after the last row)
        if row < end.y {
            result.push('\n');
        }
    }

    // Trim trailing whitespace from each line
    let trimmed: String = result
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");

    match CString::new(trimmed) {
        Ok(c) => c.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

/// Clear the current selection.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_clear_selection(handle: *mut WezTermHandle) {
    if handle.is_null() {
        return;
    }
    (*handle).inner.selection = None;
    (*handle).inner.selecting = false;
}

/// Check whether the terminal is in alternate screen mode.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_is_alt_screen(handle: *const WezTermHandle) -> bool {
    if handle.is_null() {
        return false;
    }
    (*handle).inner.terminal.is_alt_screen_active()
}

/// Check whether the terminal has grabbed the mouse.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_is_mouse_grabbed(handle: *const WezTermHandle) -> bool {
    if handle.is_null() {
        return false;
    }
    (*handle).inner.terminal.is_mouse_grabbed()
}

/// Get the current color palette.
///
/// # Parameters
/// - `out_fg`, `out_bg`: receive the default foreground/background colors
/// - `out_cursor_fg`, `out_cursor_bg`: receive cursor colors
/// - `out_ansi`: caller-provided buffer of at least 256 entries for the palette
///
/// # Safety
/// All pointers must be valid. `out_ansi` must have room for 256 entries.
#[no_mangle]
pub unsafe extern "C" fn wezterm_get_palette(
    handle: *const WezTermHandle,
    out_fg: *mut WezTermColorRGBA,
    out_bg: *mut WezTermColorRGBA,
    out_cursor_fg: *mut WezTermColorRGBA,
    out_cursor_bg: *mut WezTermColorRGBA,
    out_ansi: *mut WezTermColorRGBA,
) {
    if handle.is_null() {
        return;
    }
    let palette = (*handle).inner.terminal.palette();

    if !out_fg.is_null() {
        *out_fg = WezTermColorRGBA::from_srgba(palette.foreground);
    }
    if !out_bg.is_null() {
        *out_bg = WezTermColorRGBA::from_srgba(palette.background);
    }
    if !out_cursor_fg.is_null() {
        *out_cursor_fg = WezTermColorRGBA::from_srgba(palette.cursor_fg);
    }
    if !out_cursor_bg.is_null() {
        *out_cursor_bg = WezTermColorRGBA::from_srgba(palette.cursor_bg);
    }
    if !out_ansi.is_null() {
        for i in 0..256 {
            let color = palette.colors.0[i];
            *out_ansi.add(i) = WezTermColorRGBA::from_srgba(color);
        }
    }
}

/// Update the color palette.
///
/// # Safety
/// `handle` must be valid. `ansi_colors` must point to `ansi_count` entries.
#[no_mangle]
pub unsafe extern "C" fn wezterm_set_palette(
    handle: *mut WezTermHandle,
    fg: WezTermColorRGBA,
    bg: WezTermColorRGBA,
    cursor_fg: WezTermColorRGBA,
    cursor_bg: WezTermColorRGBA,
    ansi_colors: *const WezTermColorRGBA,
    ansi_count: u32,
) {
    if handle.is_null() {
        return;
    }
    let palette = (*handle).inner.terminal.palette_mut();
    palette.foreground = fg.to_srgba();
    palette.background = bg.to_srgba();
    palette.cursor_fg = cursor_fg.to_srgba();
    palette.cursor_bg = cursor_bg.to_srgba();

    if !ansi_colors.is_null() {
        let count = (ansi_count as usize).min(256);
        for i in 0..count {
            palette.colors.0[i] = (*ansi_colors.add(i)).to_srgba();
        }
    }
}

/// Get the current sequence number. This increments each time the terminal
/// state changes, useful for dirty-checking in the renderer.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_get_seqno(handle: *const WezTermHandle) -> u64 {
    if handle.is_null() {
        return 0;
    }
    (*handle).inner.terminal.current_seqno() as u64
}

/// Erase the scrollback buffer.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_erase_scrollback(handle: *mut WezTermHandle) {
    if handle.is_null() {
        return;
    }
    (*handle).inner.terminal.erase_scrollback();
}

/// Write raw bytes directly to the terminal's writer (PTY input).
/// Unlike `wezterm_key_down()`, this does no encoding — it sends
/// the bytes as-is.
///
/// # Safety
/// `handle` must be valid. `data` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn wezterm_write_raw(
    handle: *mut WezTermHandle,
    data: *const u8,
    len: usize,
) -> bool {
    if handle.is_null() || data.is_null() {
        return false;
    }
    let bytes = slice::from_raw_parts(data, len);
    let buffer = &(*handle).inner.output_buffer;
    buffer.lock().unwrap().extend_from_slice(bytes);
    true
}

/// Scroll the viewport by the given number of lines.
/// Positive delta scrolls up (into scrollback history).
/// Negative delta scrolls down (towards live output).
/// When the viewport reaches the bottom (live output), it resets to 0.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_scroll_viewport(handle: *mut WezTermHandle, delta: isize) {
    if handle.is_null() {
        return;
    }
    let instance = &mut (*handle).inner;
    let total_rows = instance.terminal.screen().scrollback_rows() as isize;
    let visible_rows = instance.terminal.screen().physical_rows as isize;
    let max_offset = (total_rows - visible_rows).max(0);

    let new_offset = (instance.viewport_offset + delta).clamp(0, max_offset);
    instance.viewport_offset = new_offset;
}

/// Get the current viewport offset (for scrollback viewing).
/// Returns 0 when showing live terminal output.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_get_viewport_offset(handle: *const WezTermHandle) -> isize {
    if handle.is_null() {
        return 0;
    }
    (*handle).inner.viewport_offset
}

/// Reset viewport to bottom (live terminal output).
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_scroll_to_bottom(handle: *mut WezTermHandle) {
    if handle.is_null() {
        return;
    }
    (*handle).inner.viewport_offset = 0;
}
