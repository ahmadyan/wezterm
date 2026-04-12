#!/bin/bash
set -euo pipefail

# Build libwezterm as a macOS xcframework for embedding in Agentastic.dev
#
# Produces: WezTermKit.xcframework/ containing:
#   - Static library (libwezterm_ffi.a) for arm64 + x86_64
#   - C header (libwezterm.h)
#   - module.modulemap for Swift import

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WEZTERM_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUTPUT_DIR="${SCRIPT_DIR}/build"
FRAMEWORK_NAME="WezTermKit"

echo "==> Building libwezterm for macOS..."
echo "    Workspace: ${WEZTERM_ROOT}"

# Clean previous build
rm -rf "${OUTPUT_DIR}"
mkdir -p "${OUTPUT_DIR}"

# Build for arm64 (Apple Silicon)
echo "==> Building arm64..."
cd "${WEZTERM_ROOT}"
cargo build -p libwezterm --release --target aarch64-apple-darwin 2>&1

# Build for x86_64 (Intel)
echo "==> Building x86_64..."
cargo build -p libwezterm --release --target x86_64-apple-darwin 2>&1

# Create universal (fat) static library
echo "==> Creating universal binary..."
LIPO_OUTPUT="${OUTPUT_DIR}/libwezterm_ffi.a"
lipo -create \
    "${WEZTERM_ROOT}/target/aarch64-apple-darwin/release/libwezterm_ffi.a" \
    "${WEZTERM_ROOT}/target/x86_64-apple-darwin/release/libwezterm_ffi.a" \
    -output "${LIPO_OUTPUT}"

echo "    Universal library: $(du -h "${LIPO_OUTPUT}" | cut -f1)"
lipo -info "${LIPO_OUTPUT}"

# Copy the generated C header
echo "==> Copying header..."
mkdir -p "${OUTPUT_DIR}/include"
cp "${SCRIPT_DIR}/include/libwezterm.h" "${OUTPUT_DIR}/include/"

# Generate a C-compatible header wrapper (Swift-friendly)
cat > "${OUTPUT_DIR}/include/wezterm.h" << 'HEADER'
/* WezTermKit - C API for WezTerm terminal emulation */
/* This header wraps libwezterm.h for Swift/ObjC consumption */

#ifndef WEZTERMKIT_H
#define WEZTERMKIT_H

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* --- Constants --- */

#define WEZTERM_MOD_NONE   0
#define WEZTERM_MOD_SHIFT  (1 << 0)
#define WEZTERM_MOD_ALT    (1 << 1)
#define WEZTERM_MOD_CTRL   (1 << 2)
#define WEZTERM_MOD_SUPER  (1 << 3)

/* --- Enums --- */

typedef enum {
    WezTermCursorShapeDefault = 0,
    WezTermCursorShapeBlinkingBlock,
    WezTermCursorShapeSteadyBlock,
    WezTermCursorShapeBlinkingUnderline,
    WezTermCursorShapeSteadyUnderline,
    WezTermCursorShapeBlinkingBar,
    WezTermCursorShapeSteadyBar,
} WezTermCursorShape;

typedef enum {
    WezTermKeyCodeChar = 0,
    WezTermKeyCodeBackspace,
    WezTermKeyCodeTab,
    WezTermKeyCodeEnter,
    WezTermKeyCodeEscape,
    WezTermKeyCodePageUp,
    WezTermKeyCodePageDown,
    WezTermKeyCodeEnd,
    WezTermKeyCodeHome,
    WezTermKeyCodeLeftArrow,
    WezTermKeyCodeRightArrow,
    WezTermKeyCodeUpArrow,
    WezTermKeyCodeDownArrow,
    WezTermKeyCodeInsert,
    WezTermKeyCodeDelete,
    WezTermKeyCodeF1,
    WezTermKeyCodeF2,
    WezTermKeyCodeF3,
    WezTermKeyCodeF4,
    WezTermKeyCodeF5,
    WezTermKeyCodeF6,
    WezTermKeyCodeF7,
    WezTermKeyCodeF8,
    WezTermKeyCodeF9,
    WezTermKeyCodeF10,
    WezTermKeyCodeF11,
    WezTermKeyCodeF12,
} WezTermKeyCode;

typedef enum {
    WezTermMouseButtonLeft = 0,
    WezTermMouseButtonMiddle,
    WezTermMouseButtonRight,
    WezTermMouseButtonWheelUp,
    WezTermMouseButtonWheelDown,
    WezTermMouseButtonNone,
} WezTermMouseButton;

typedef enum {
    WezTermMouseEventKindPress = 0,
    WezTermMouseEventKindRelease,
    WezTermMouseEventKindMove,
} WezTermMouseEventKind;

typedef enum {
    WezTermUnderlineNone = 0,
    WezTermUnderlineSingle,
    WezTermUnderlineDouble,
    WezTermUnderlineCurly,
    WezTermUnderlineDotted,
    WezTermUnderlineDashed,
} WezTermUnderline;

/* --- Structs --- */

typedef struct WezTermHandle WezTermHandle;

typedef struct {
    uint8_t r;
    uint8_t g;
    uint8_t b;
    uint8_t a;
} WezTermColorRGBA;

typedef struct {
    uint32_t scrollback_size;
    WezTermColorRGBA foreground;
    WezTermColorRGBA background;
    WezTermColorRGBA cursor_fg;
    WezTermColorRGBA cursor_bg;
    const WezTermColorRGBA *ansi_colors;
    uint32_t ansi_color_count;
    bool enable_kitty_graphics;
    bool enable_kitty_keyboard;
} WezTermConfig;

typedef struct {
    void (*on_title_changed)(void *ctx, const char *title);
    void (*on_cwd_changed)(void *ctx);
    void (*on_bell)(void *ctx);
} WezTermCallbacks;

typedef struct {
    uint32_t x;
    int32_t y;
    WezTermCursorShape shape;
    bool visible;
} WezTermCursorInfo;

typedef struct {
    uint8_t text[8];
    uint8_t text_len;
    uint8_t width;
    WezTermColorRGBA fg;
    WezTermColorRGBA bg;
    bool bold;
    bool italic;
    WezTermUnderline underline;
    bool strikethrough;
    bool reverse;
    bool invisible;
} WezTermCellInfo;

/* --- Lifecycle --- */

void wezterm_init(void);

WezTermHandle *wezterm_new(
    uint32_t rows,
    uint32_t cols,
    uint32_t pixel_width,
    uint32_t pixel_height,
    const WezTermConfig *config,
    WezTermCallbacks callbacks,
    void *callback_context
);

void wezterm_free(WezTermHandle *handle);

/* --- Data I/O --- */

void wezterm_advance_bytes(WezTermHandle *handle, const uint8_t *data, size_t len);
void wezterm_take_output(WezTermHandle *handle, uint8_t **out_data, size_t *out_len);
void wezterm_free_bytes(uint8_t *data);
bool wezterm_write_raw(WezTermHandle *handle, const uint8_t *data, size_t len);

/* --- Terminal State --- */

void wezterm_resize(WezTermHandle *handle, uint32_t rows, uint32_t cols,
                    uint32_t pixel_width, uint32_t pixel_height);
void wezterm_get_cursor(const WezTermHandle *handle, WezTermCursorInfo *out);
uint32_t wezterm_get_visible_rows(const WezTermHandle *handle);
uint32_t wezterm_get_visible_cols(const WezTermHandle *handle);
uint32_t wezterm_get_total_rows(const WezTermHandle *handle);
uint32_t wezterm_get_row_cells(WezTermHandle *handle, int32_t row,
                               WezTermCellInfo *out_cells, uint32_t max_cells);
uint32_t wezterm_get_scrollback_row_cells(WezTermHandle *handle, intptr_t stable_row,
                                          WezTermCellInfo *out_cells, uint32_t max_cells);
uint64_t wezterm_get_seqno(const WezTermHandle *handle);
bool wezterm_is_alt_screen(const WezTermHandle *handle);
bool wezterm_is_mouse_grabbed(const WezTermHandle *handle);

/* --- Input --- */

bool wezterm_key_down(WezTermHandle *handle, WezTermKeyCode key, uint32_t modifiers);
bool wezterm_key_up(WezTermHandle *handle, WezTermKeyCode key, uint32_t modifiers);
bool wezterm_mouse_event(WezTermHandle *handle, WezTermMouseEventKind kind,
                         uint32_t x, int32_t y, WezTermMouseButton button, uint32_t modifiers);
bool wezterm_send_paste(WezTermHandle *handle, const char *text);
void wezterm_focus_changed(WezTermHandle *handle, bool focused);

/* --- Queries --- */

char *wezterm_get_title(const WezTermHandle *handle);
char *wezterm_get_current_dir(const WezTermHandle *handle);
void wezterm_free_string(char *s);

/* --- Colors --- */

void wezterm_get_palette(const WezTermHandle *handle,
                         WezTermColorRGBA *out_fg, WezTermColorRGBA *out_bg,
                         WezTermColorRGBA *out_cursor_fg, WezTermColorRGBA *out_cursor_bg,
                         WezTermColorRGBA *out_ansi);
void wezterm_set_palette(WezTermHandle *handle,
                         WezTermColorRGBA fg, WezTermColorRGBA bg,
                         WezTermColorRGBA cursor_fg, WezTermColorRGBA cursor_bg,
                         const WezTermColorRGBA *ansi_colors, uint32_t ansi_count);
void wezterm_erase_scrollback(WezTermHandle *handle);

#ifdef __cplusplus
}
#endif

#endif /* WEZTERMKIT_H */
HEADER

# Create module map for Swift
cat > "${OUTPUT_DIR}/include/module.modulemap" << 'MODULEMAP'
module WezTermKit [system] {
    header "wezterm.h"
    export *
}
MODULEMAP

# Package as xcframework
echo "==> Creating xcframework..."
XCFW_DIR="${OUTPUT_DIR}/${FRAMEWORK_NAME}.xcframework"
rm -rf "${XCFW_DIR}"

xcodebuild -create-xcframework \
    -library "${LIPO_OUTPUT}" \
    -headers "${OUTPUT_DIR}/include" \
    -output "${XCFW_DIR}"

echo ""
echo "==> Build complete!"
echo "    Framework: ${XCFW_DIR}"
echo ""
echo "    To integrate:"
echo "    1. Drag ${FRAMEWORK_NAME}.xcframework into Xcode"
echo "    2. import WezTermKit in Swift files"
du -sh "${XCFW_DIR}"
