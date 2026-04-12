//! C-compatible type definitions for the libwezterm FFI.

use std::os::raw::c_char;
use wezterm_color_types::SrgbaTuple;
use wezterm_surface::CursorShape;

// ---------------------------------------------------------------------------
// Color
// ---------------------------------------------------------------------------

/// RGBA color with 8-bit components.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct WezTermColorRGBA {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl WezTermColorRGBA {
    pub fn from_srgba(t: SrgbaTuple) -> Self {
        let (r, g, b, a) = t.as_rgba_u8();
        Self { r, g, b, a }
    }

    pub fn to_srgba(self) -> SrgbaTuple {
        SrgbaTuple(
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        )
    }
}

// ---------------------------------------------------------------------------
// Cursor
// ---------------------------------------------------------------------------

/// Cursor shape variants.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WezTermCursorShape {
    Default,
    BlinkingBlock,
    SteadyBlock,
    BlinkingUnderline,
    SteadyUnderline,
    BlinkingBar,
    SteadyBar,
}

impl From<CursorShape> for WezTermCursorShape {
    fn from(shape: CursorShape) -> Self {
        match shape {
            CursorShape::Default => Self::Default,
            CursorShape::BlinkingBlock => Self::BlinkingBlock,
            CursorShape::SteadyBlock => Self::SteadyBlock,
            CursorShape::BlinkingUnderline => Self::BlinkingUnderline,
            CursorShape::SteadyUnderline => Self::SteadyUnderline,
            CursorShape::BlinkingBar => Self::BlinkingBar,
            CursorShape::SteadyBar => Self::SteadyBar,
        }
    }
}

/// Cursor state information.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WezTermCursorInfo {
    pub x: u32,
    pub y: i32,
    pub shape: WezTermCursorShape,
    pub visible: bool,
}

// ---------------------------------------------------------------------------
// Cell attributes
// ---------------------------------------------------------------------------

/// Underline style.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WezTermUnderline {
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

impl From<wezterm_cell::Underline> for WezTermUnderline {
    fn from(u: wezterm_cell::Underline) -> Self {
        match u {
            wezterm_cell::Underline::None => Self::None,
            wezterm_cell::Underline::Single => Self::Single,
            wezterm_cell::Underline::Double => Self::Double,
            wezterm_cell::Underline::Curly => Self::Curly,
            wezterm_cell::Underline::Dotted => Self::Dotted,
            wezterm_cell::Underline::Dashed => Self::Dashed,
        }
    }
}

/// Text intensity (normal, bold, or faint).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WezTermIntensity {
    Normal,
    Bold,
    Faint,
}

/// Information about a single cell in a terminal line.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WezTermCellInfo {
    /// UTF-8 encoded text of this cell's grapheme cluster.
    /// Up to 7 bytes (most graphemes fit). NUL-padded.
    pub text: [u8; 8],
    /// Length of valid UTF-8 bytes in `text`.
    pub text_len: u8,
    /// Display width of this cell (1 for normal, 2 for wide characters).
    pub width: u8,
    /// Foreground color (resolved from palette).
    pub fg: WezTermColorRGBA,
    /// Background color (resolved from palette).
    pub bg: WezTermColorRGBA,
    /// Whether the cell is bold.
    pub bold: bool,
    /// Whether the cell is italic.
    pub italic: bool,
    /// Underline style.
    pub underline: WezTermUnderline,
    /// Whether the cell has strikethrough.
    pub strikethrough: bool,
    /// Whether the cell's colors are reversed.
    pub reverse: bool,
    /// Whether the cell is invisible.
    pub invisible: bool,
}

impl WezTermCellInfo {
    pub fn blank() -> Self {
        Self {
            text: [b' ', 0, 0, 0, 0, 0, 0, 0],
            text_len: 1,
            width: 1,
            fg: WezTermColorRGBA::default(),
            bg: WezTermColorRGBA::default(),
            bold: false,
            italic: false,
            underline: WezTermUnderline::None,
            strikethrough: false,
            reverse: false,
            invisible: false,
        }
    }
}

/// Information about a line (row) in the terminal.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WezTermLineInfo {
    /// Whether this line was wrapped from the previous line.
    pub wrapped: bool,
    /// The sequence number when this line was last changed.
    pub seqno: u64,
}

// ---------------------------------------------------------------------------
// Callbacks
// ---------------------------------------------------------------------------

/// Callback function types for terminal events.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WezTermCallbacks {
    /// Called when the terminal title changes (OSC 0/2).
    pub on_title_changed: Option<unsafe extern "C" fn(ctx: *mut libc::c_void, title: *const c_char)>,
    /// Called when the working directory changes (OSC 7).
    pub on_cwd_changed: Option<unsafe extern "C" fn(ctx: *mut libc::c_void)>,
    /// Called when the terminal bell rings.
    pub on_bell: Option<unsafe extern "C" fn(ctx: *mut libc::c_void)>,
}

impl Default for WezTermCallbacks {
    fn default() -> Self {
        Self {
            on_title_changed: None,
            on_cwd_changed: None,
            on_bell: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for creating a new terminal instance.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct WezTermConfig {
    /// Scrollback buffer size in lines. 0 = use default (3500).
    pub scrollback_size: u32,
    /// Default foreground color.
    pub foreground: WezTermColorRGBA,
    /// Default background color.
    pub background: WezTermColorRGBA,
    /// Cursor foreground color.
    pub cursor_fg: WezTermColorRGBA,
    /// Cursor background color.
    pub cursor_bg: WezTermColorRGBA,
    /// ANSI color palette (16 entries). NULL = use defaults.
    pub ansi_colors: *const WezTermColorRGBA,
    /// Number of entries in ansi_colors (max 256).
    pub ansi_color_count: u32,
    /// Enable Kitty graphics protocol.
    pub enable_kitty_graphics: bool,
    /// Enable Kitty keyboard protocol.
    pub enable_kitty_keyboard: bool,
}

impl Default for WezTermConfig {
    fn default() -> Self {
        Self {
            scrollback_size: 3500,
            foreground: WezTermColorRGBA { r: 204, g: 204, b: 204, a: 255 },
            background: WezTermColorRGBA { r: 0, g: 0, b: 0, a: 255 },
            cursor_fg: WezTermColorRGBA { r: 0, g: 0, b: 0, a: 255 },
            cursor_bg: WezTermColorRGBA { r: 82, g: 82, b: 82, a: 255 },
            ansi_colors: std::ptr::null(),
            ansi_color_count: 0,
            enable_kitty_graphics: false,
            enable_kitty_keyboard: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

/// Key code for keyboard input.
/// Matches a subset of termwiz::input::KeyCode.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WezTermKeyCode {
    /// A unicode character.
    Char,
    Backspace,
    Tab,
    Enter,
    Escape,
    PageUp,
    PageDown,
    End,
    Home,
    LeftArrow,
    RightArrow,
    UpArrow,
    DownArrow,
    Insert,
    Delete,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

/// Key code with an associated character value (for Char variant).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WezTermKeyEvent {
    pub code: WezTermKeyCode,
    /// The unicode character, if code == Char.
    pub char_value: u32,
    /// Modifier flags (bitmask of WEZTERM_MOD_*).
    pub modifiers: u32,
}

// Modifier flags matching termwiz::input::Modifiers
pub const WEZTERM_MOD_NONE: u32 = 0;
pub const WEZTERM_MOD_SHIFT: u32 = 1 << 0;
pub const WEZTERM_MOD_ALT: u32 = 1 << 1;
pub const WEZTERM_MOD_CTRL: u32 = 1 << 2;
pub const WEZTERM_MOD_SUPER: u32 = 1 << 3;

impl WezTermKeyCode {
    pub fn to_termwiz(self) -> termwiz::input::KeyCode {
        match self {
            Self::Char => termwiz::input::KeyCode::Char(' '), // placeholder, overridden by char_value
            Self::Backspace => termwiz::input::KeyCode::Backspace,
            Self::Tab => termwiz::input::KeyCode::Tab,
            Self::Enter => termwiz::input::KeyCode::Enter,
            Self::Escape => termwiz::input::KeyCode::Escape,
            Self::PageUp => termwiz::input::KeyCode::PageUp,
            Self::PageDown => termwiz::input::KeyCode::PageDown,
            Self::End => termwiz::input::KeyCode::End,
            Self::Home => termwiz::input::KeyCode::Home,
            Self::LeftArrow => termwiz::input::KeyCode::LeftArrow,
            Self::RightArrow => termwiz::input::KeyCode::RightArrow,
            Self::UpArrow => termwiz::input::KeyCode::UpArrow,
            Self::DownArrow => termwiz::input::KeyCode::DownArrow,
            Self::Insert => termwiz::input::KeyCode::Insert,
            Self::Delete => termwiz::input::KeyCode::Delete,
            Self::F1 => termwiz::input::KeyCode::Function(1),
            Self::F2 => termwiz::input::KeyCode::Function(2),
            Self::F3 => termwiz::input::KeyCode::Function(3),
            Self::F4 => termwiz::input::KeyCode::Function(4),
            Self::F5 => termwiz::input::KeyCode::Function(5),
            Self::F6 => termwiz::input::KeyCode::Function(6),
            Self::F7 => termwiz::input::KeyCode::Function(7),
            Self::F8 => termwiz::input::KeyCode::Function(8),
            Self::F9 => termwiz::input::KeyCode::Function(9),
            Self::F10 => termwiz::input::KeyCode::Function(10),
            Self::F11 => termwiz::input::KeyCode::Function(11),
            Self::F12 => termwiz::input::KeyCode::Function(12),
        }
    }
}

/// Mouse button for mouse events.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WezTermMouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    None,
}

impl WezTermMouseButton {
    pub fn to_termwiz(self) -> wezterm_term::input::MouseButton {
        match self {
            Self::Left => wezterm_term::input::MouseButton::Left,
            Self::Middle => wezterm_term::input::MouseButton::Middle,
            Self::Right => wezterm_term::input::MouseButton::Right,
            Self::WheelUp => wezterm_term::input::MouseButton::WheelUp(1),
            Self::WheelDown => wezterm_term::input::MouseButton::WheelDown(1),
            Self::None => wezterm_term::input::MouseButton::None,
        }
    }
}

/// Mouse event kind.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WezTermMouseEventKind {
    Press,
    Release,
    Move,
}

impl WezTermMouseEventKind {
    pub fn to_termwiz(self) -> wezterm_term::input::MouseEventKind {
        match self {
            Self::Press => wezterm_term::input::MouseEventKind::Press,
            Self::Release => wezterm_term::input::MouseEventKind::Release,
            Self::Move => wezterm_term::input::MouseEventKind::Move,
        }
    }
}
