//! Embed API stubs.
//!
//! The embed API (wezterm_embed_init, wezterm_embed_spawn_window) is deprecated.
//! Use the surface API (wezterm_surface_new, wezterm_surface_render) instead.
//! These stubs are kept so the C header and existing callers compile without
//! link errors.

use std::os::raw::{c_char, c_void};

/// Callback type for embed window ready notification.
pub type WezTermWindowReadyCallback =
    Option<unsafe extern "C" fn(userdata: *mut c_void, nswindow: *mut c_void)>;

/// Deprecated. Returns false. Use the surface API instead.
#[no_mangle]
pub extern "C" fn wezterm_embed_init() -> bool {
    false
}

/// Deprecated. Does nothing. Use the surface API instead.
#[no_mangle]
pub unsafe extern "C" fn wezterm_embed_spawn_window(
    _pixel_width: u32,
    _pixel_height: u32,
    _command: *const c_char,
    _cwd: *const c_char,
    callback: WezTermWindowReadyCallback,
    userdata: *mut c_void,
) {
    // Fire callback with NULL to indicate failure.
    if let Some(cb) = callback {
        cb(userdata, std::ptr::null_mut());
    }
}
