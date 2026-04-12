#include <cstdarg>
#include <cstdint>
#include <cstdlib>
#include <ostream>
#include <new>

constexpr static const uint32_t WEZTERM_MOD_NONE = 0;

constexpr static const uint32_t WEZTERM_MOD_SHIFT = (1 << 0);

constexpr static const uint32_t WEZTERM_MOD_ALT = (1 << 1);

constexpr static const uint32_t WEZTERM_MOD_CTRL = (1 << 2);

constexpr static const uint32_t WEZTERM_MOD_SUPER = (1 << 3);

/// Cursor shape variants.
enum class WezTermCursorShape {
  Default,
  BlinkingBlock,
  SteadyBlock,
  BlinkingUnderline,
  SteadyUnderline,
  BlinkingBar,
  SteadyBar,
};

/// Key code for keyboard input.
/// Matches a subset of termwiz::input::KeyCode.
enum class WezTermKeyCode {
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
};

/// Mouse button for mouse events.
enum class WezTermMouseButton {
  Left,
  Middle,
  Right,
  WheelUp,
  WheelDown,
  None,
};

/// Mouse event kind.
enum class WezTermMouseEventKind {
  Press,
  Release,
  Move,
};

/// Underline style.
enum class WezTermUnderline {
  None,
  Single,
  Double,
  Curly,
  Dotted,
  Dashed,
};

/// Opaque handle to a WezTerm terminal instance.
/// Created by `wezterm_new()`, freed by `wezterm_free()`.
struct WezTermHandle;

/// Opaque handle to a GPU rendering surface.
struct WezTermSurface;

/// RGBA color with 8-bit components.
struct WezTermColorRGBA {
  uint8_t r;
  uint8_t g;
  uint8_t b;
  uint8_t a;
};

/// Configuration for creating a new terminal instance.
struct WezTermConfig {
  /// Scrollback buffer size in lines. 0 = use default (3500).
  uint32_t scrollback_size;
  /// Default foreground color.
  WezTermColorRGBA foreground;
  /// Default background color.
  WezTermColorRGBA background;
  /// Cursor foreground color.
  WezTermColorRGBA cursor_fg;
  /// Cursor background color.
  WezTermColorRGBA cursor_bg;
  /// ANSI color palette (16 entries). NULL = use defaults.
  const WezTermColorRGBA *ansi_colors;
  /// Number of entries in ansi_colors (max 256).
  uint32_t ansi_color_count;
  /// Enable Kitty graphics protocol.
  bool enable_kitty_graphics;
  /// Enable Kitty keyboard protocol.
  bool enable_kitty_keyboard;
};

/// Callback function types for terminal events.
struct WezTermCallbacks {
  /// Called when the terminal title changes (OSC 0/2).
  void (*on_title_changed)(void *ctx, const char *title);
  /// Called when the working directory changes (OSC 7).
  void (*on_cwd_changed)(void *ctx);
  /// Called when the terminal bell rings.
  void (*on_bell)(void *ctx);
};

/// Cursor state information.
struct WezTermCursorInfo {
  uint32_t x;
  int32_t y;
  WezTermCursorShape shape;
  bool visible;
};

/// Information about a single cell in a terminal line.
struct WezTermCellInfo {
  /// UTF-8 encoded text of this cell's grapheme cluster.
  /// Up to 7 bytes (most graphemes fit). NUL-padded.
  uint8_t text[8];
  /// Length of valid UTF-8 bytes in `text`.
  uint8_t text_len;
  /// Display width of this cell (1 for normal, 2 for wide characters).
  uint8_t width;
  /// Foreground color (resolved from palette).
  WezTermColorRGBA fg;
  /// Background color (resolved from palette).
  WezTermColorRGBA bg;
  /// Whether the cell is bold.
  bool bold;
  /// Whether the cell is italic.
  bool italic;
  /// Underline style.
  WezTermUnderline underline;
  /// Whether the cell has strikethrough.
  bool strikethrough;
  /// Whether the cell's colors are reversed.
  bool reverse;
  /// Whether the cell is invisible.
  bool invisible;
};

/// Callback type for embed window ready notification.
using WezTermWindowReadyCallback = void(*)(void *userdata, void *nswindow);

extern "C" {

/// Initialize the library. Call once before any other function.
/// Sets up logging if WEZTERM_LOG env var is set.
void wezterm_init();

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
WezTermHandle *wezterm_new(uint32_t rows,
                           uint32_t cols,
                           uint32_t pixel_width,
                           uint32_t pixel_height,
                           const WezTermConfig *config,
                           WezTermCallbacks callbacks,
                           void *callback_context);

/// Free a terminal instance and all associated resources.
///
/// # Safety
/// `handle` must be a valid pointer returned by `wezterm_new()`, or NULL.
/// After this call, the handle is invalid and must not be used.
void wezterm_free(WezTermHandle *handle);

/// Feed bytes from the PTY output into the terminal emulator.
/// This is the primary way to update the terminal state — call this
/// whenever the PTY produces output.
///
/// # Safety
/// `handle` must be valid. `data` must point to `len` readable bytes.
void wezterm_advance_bytes(WezTermHandle *handle, const uint8_t *data, uintptr_t len);

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
void wezterm_take_output(WezTermHandle *handle, uint8_t **out_data, uintptr_t *out_len);

/// Free a byte buffer returned by `wezterm_take_output()`.
///
/// # Safety
/// `data` must be a pointer returned by `wezterm_take_output()`, or NULL.
void wezterm_free_bytes(uint8_t *data);

/// Resize the terminal.
///
/// # Safety
/// `handle` must be valid.
void wezterm_resize(WezTermHandle *handle,
                    uint32_t rows,
                    uint32_t cols,
                    uint32_t pixel_width,
                    uint32_t pixel_height);

/// Get the current cursor position and shape.
///
/// # Safety
/// `handle` and `out` must be valid pointers.
void wezterm_get_cursor(const WezTermHandle *handle, WezTermCursorInfo *out);

/// Get the number of visible rows in the terminal.
///
/// # Safety
/// `handle` must be valid.
uint32_t wezterm_get_visible_rows(const WezTermHandle *handle);

/// Get the number of visible columns in the terminal.
///
/// # Safety
/// `handle` must be valid.
uint32_t wezterm_get_visible_cols(const WezTermHandle *handle);

/// Get the total number of lines (scrollback + visible).
///
/// # Safety
/// `handle` must be valid.
uint32_t wezterm_get_total_rows(const WezTermHandle *handle);

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
uint32_t wezterm_get_row_cells(WezTermHandle *handle,
                               int32_t row,
                               WezTermCellInfo *out_cells,
                               uint32_t max_cells);

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
uint32_t wezterm_get_scrollback_row_cells(WezTermHandle *handle,
                                          intptr_t stable_row,
                                          WezTermCellInfo *out_cells,
                                          uint32_t max_cells);

/// Send a key-down event to the terminal. The terminal encodes this
/// into the appropriate escape sequence and buffers it for output.
/// Use `wezterm_take_output()` to retrieve the encoded bytes.
///
/// # Safety
/// `handle` must be valid.
bool wezterm_key_down(WezTermHandle *handle, WezTermKeyCode key, uint32_t modifiers);

/// Send a key-down event for a printable character. Unlike `wezterm_key_down`,
/// this preserves the original codepoint so the terminal can encode modifier
/// combinations correctly instead of forcing the caller down the raw-byte path.
///
/// # Safety
/// `handle` must be valid.
bool wezterm_key_down_char(WezTermHandle *handle, uint32_t ch, uint32_t modifiers);

/// Send a key-up event to the terminal (used by Kitty keyboard protocol).
///
/// # Safety
/// `handle` must be valid.
bool wezterm_key_up(WezTermHandle *handle, WezTermKeyCode key, uint32_t modifiers);

/// Send a mouse event to the terminal.
///
/// # Safety
/// `handle` must be valid.
bool wezterm_mouse_event(WezTermHandle *handle,
                         WezTermMouseEventKind kind,
                         uint32_t x,
                         int32_t y,
                         WezTermMouseButton button,
                         uint32_t modifiers);

/// Send a paste operation to the terminal.
/// Handles bracketed paste mode automatically.
///
/// # Safety
/// `handle` must be valid. `text` must be a valid UTF-8 C string.
bool wezterm_send_paste(WezTermHandle *handle, const char *text);

/// Send committed UTF-8 text directly to the terminal's writer.
/// Unlike `wezterm_send_paste()`, this does not wrap the text in bracketed-paste
/// sequences; it is intended for normal text input that was already composed by
/// the host input method.
///
/// # Safety
/// `handle` must be valid. `text` must be a valid UTF-8 C string.
bool wezterm_send_text(WezTermHandle *handle, const char *text);

/// Notify the terminal that focus has changed.
///
/// # Safety
/// `handle` must be valid.
void wezterm_focus_changed(WezTermHandle *handle, bool focused);

/// Get the current terminal title (set via OSC escape sequences).
///
/// # Returns
/// A newly allocated C string, or NULL if no title is set.
/// The caller must free it with `wezterm_free_string()`.
///
/// # Safety
/// `handle` must be valid.
char *wezterm_get_title(const WezTermHandle *handle);

/// Get the current working directory (set via OSC 7).
///
/// # Returns
/// A newly allocated C string, or NULL if no CWD is set.
/// The caller must free it with `wezterm_free_string()`.
///
/// # Safety
/// `handle` must be valid.
char *wezterm_get_current_dir(const WezTermHandle *handle);

/// Free a string returned by `wezterm_get_title()` or `wezterm_get_current_dir()`.
///
/// # Safety
/// `s` must be a pointer returned by one of the string-returning functions, or NULL.
void wezterm_free_string(char *s);

/// Check whether the terminal is in alternate screen mode.
///
/// # Safety
/// `handle` must be valid.
bool wezterm_is_alt_screen(const WezTermHandle *handle);

/// Check whether the terminal has grabbed the mouse.
///
/// # Safety
/// `handle` must be valid.
bool wezterm_is_mouse_grabbed(const WezTermHandle *handle);

/// Get the current color palette.
///
/// # Parameters
/// - `out_fg`, `out_bg`: receive the default foreground/background colors
/// - `out_cursor_fg`, `out_cursor_bg`: receive cursor colors
/// - `out_ansi`: caller-provided buffer of at least 256 entries for the palette
///
/// # Safety
/// All pointers must be valid. `out_ansi` must have room for 256 entries.
void wezterm_get_palette(const WezTermHandle *handle,
                         WezTermColorRGBA *out_fg,
                         WezTermColorRGBA *out_bg,
                         WezTermColorRGBA *out_cursor_fg,
                         WezTermColorRGBA *out_cursor_bg,
                         WezTermColorRGBA *out_ansi);

/// Update the color palette.
///
/// # Safety
/// `handle` must be valid. `ansi_colors` must point to `ansi_count` entries.
void wezterm_set_palette(WezTermHandle *handle,
                         WezTermColorRGBA fg,
                         WezTermColorRGBA bg,
                         WezTermColorRGBA cursor_fg,
                         WezTermColorRGBA cursor_bg,
                         const WezTermColorRGBA *ansi_colors,
                         uint32_t ansi_count);

/// Get the current sequence number. This increments each time the terminal
/// state changes, useful for dirty-checking in the renderer.
///
/// # Safety
/// `handle` must be valid.
uint64_t wezterm_get_seqno(const WezTermHandle *handle);

/// Erase the scrollback buffer.
///
/// # Safety
/// `handle` must be valid.
void wezterm_erase_scrollback(WezTermHandle *handle);

/// Write raw bytes directly to the terminal's writer (PTY input).
/// Unlike `wezterm_key_down()`, this does no encoding — it sends
/// the bytes as-is.
///
/// # Safety
/// `handle` must be valid. `data` must point to `len` readable bytes.
bool wezterm_write_raw(WezTermHandle *handle, const uint8_t *data, uintptr_t len);

/// Deprecated. Returns false. Use the surface API instead.
bool wezterm_embed_init();

/// Deprecated. Does nothing. Use the surface API instead.
void wezterm_embed_spawn_window(uint32_t _pixel_width,
                                uint32_t _pixel_height,
                                const char *_command,
                                const char *_cwd,
                                WezTermWindowReadyCallback callback,
                                void *userdata);

/// Create a GPU rendering surface for a terminal.
///
/// The surface uses wgpu (Metal on macOS) for GPU-accelerated rendering
/// with WezTerm's font system (freetype/harfbuzz) for text shaping.
///
/// # Parameters
/// - `terminal`: The terminal instance to render
/// - `nsview`: Pointer to the NSView that will display the terminal
/// - `scale_factor`: Display scale factor (e.g., 2.0 for Retina)
/// - `width`, `height`: Surface dimensions in pixels
///
/// # Safety
/// `terminal` and `nsview` must be valid pointers.
WezTermSurface *wezterm_surface_new(WezTermHandle *terminal,
                                    void *nsview,
                                    double scale_factor,
                                    uint32_t width,
                                    uint32_t height);

/// Free a GPU rendering surface.
///
/// # Safety
/// `surface` must be a valid pointer returned by `wezterm_surface_new()`, or NULL.
void wezterm_surface_free(WezTermSurface *surface);

/// Render the terminal content to the GPU surface.
///
/// Call this from the display link or when the terminal state changes.
///
/// # Safety
/// `surface` and `terminal` must be valid pointers.
bool wezterm_surface_render(WezTermSurface *surface,
                            WezTermHandle *terminal,
                            float bg_r,
                            float bg_g,
                            float bg_b,
                            float bg_a,
                            float cursor_r,
                            float cursor_g,
                            float cursor_b,
                            float cursor_a);

/// Resize the GPU rendering surface.
///
/// # Safety
/// `surface` must be valid.
void wezterm_surface_resize(WezTermSurface *surface,
                            uint32_t width,
                            uint32_t height,
                            double scale_factor);

/// Update the surface font and DPI-derived metrics.
///
/// # Safety
/// `surface` must be valid. `font_family` may be NULL.
bool wezterm_surface_set_font(WezTermSurface *surface,
                              const char *font_family,
                              double font_size,
                              double scale_factor);

/// Get the cell dimensions (in pixels) for the current font.
///
/// # Safety
/// `surface`, `out_width`, and `out_height` must be valid.
void wezterm_surface_get_cell_size(const WezTermSurface *surface,
                                   double *out_width,
                                   double *out_height);

}  // extern "C"
