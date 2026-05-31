//! C FFI surface API for GPU-rendered terminal.
//!
//! Provides functions to create a wgpu rendering surface from an NSView,
//! render terminal content via Metal, and manage font/palette settings.

use crate::font_config;
use crate::render::gpu::GpuRenderer;
use crate::WezTermHandle;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::rc::Rc;

use config::ConfigHandle;
use wezterm_font::FontConfiguration;

/// Opaque handle to a GPU rendering surface.
pub struct WezTermSurface {
    renderer: GpuRenderer,
    config: ConfigHandle,
    font_config: Rc<FontConfiguration>,
}

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
#[no_mangle]
pub unsafe extern "C" fn wezterm_surface_new(
    terminal: *mut WezTermHandle,
    nsview: *mut libc::c_void,
    scale_factor: f64,
    width: u32,
    height: u32,
) -> *mut WezTermSurface {
    if terminal.is_null() || nsview.is_null() || width == 0 || height == 0 {
        return std::ptr::null_mut();
    }

    let nsview_addr = nsview as usize;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> anyhow::Result<*mut WezTermSurface> {
        let nsview = nsview_addr as *mut libc::c_void;
        let dpi = (scale_factor * 72.0) as usize;

        let (font_config, config) = font_config::make_font_config(dpi, None, None)?;

        let renderer = GpuRenderer::new(
            nsview,
            width,
            height,
            scale_factor,
            &config,
            Rc::clone(&font_config),
        )?;

        let surface = Box::new(WezTermSurface {
            renderer,
            config,
            font_config,
        });
        Ok(Box::into_raw(surface))
    }));

    match result {
        Ok(Ok(ptr)) => ptr,
        Ok(Err(e)) => {
            let msg = format!("wezterm_surface_new failed: {:#}", e);
            log::error!("{}", msg);
            eprintln!("{}", msg);
            let _ = std::fs::write("/tmp/agentastic-wezterm-surface.log", &msg);
            std::ptr::null_mut()
        }
        Err(panic) => {
            let msg = match panic.downcast_ref::<&str>() {
                Some(s) => format!("wezterm_surface_new panicked: {}", s),
                None => match panic.downcast_ref::<String>() {
                    Some(s) => format!("wezterm_surface_new panicked: {}", s),
                    None => "wezterm_surface_new panicked: <unknown payload>".to_string(),
                },
            };
            log::error!("{}", msg);
            eprintln!("{}", msg);
            let _ = std::fs::write("/tmp/agentastic-wezterm-surface.log", &msg);
            std::ptr::null_mut()
        }
    }
}

/// Free a GPU rendering surface.
///
/// # Safety
/// `surface` must be a valid pointer returned by `wezterm_surface_new()`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn wezterm_surface_free(surface: *mut WezTermSurface) {
    if !surface.is_null() {
        drop(Box::from_raw(surface));
    }
}

/// Render the terminal content to the GPU surface.
///
/// Call this from the display link or when the terminal state changes.
///
/// # Safety
/// `surface` and `terminal` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn wezterm_surface_render(
    surface: *mut WezTermSurface,
    terminal: *mut WezTermHandle,
    bg_r: f32,
    bg_g: f32,
    bg_b: f32,
    bg_a: f32,
    cursor_r: f32,
    cursor_g: f32,
    cursor_b: f32,
    cursor_a: f32,
) -> bool {
    if surface.is_null() || terminal.is_null() {
        return false;
    }

    // Catch panics so we can log them — the panic_cannot_unwind shim
    // converts panics to abort() across FFI boundaries by default, hiding
    // the actual error.
    let surface_ptr = surface;
    let terminal_ptr = terminal;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let surface = &mut *surface_ptr;
        let instance = &mut (*terminal_ptr).inner;
        let viewport_offset = instance.viewport_offset;
        let bg_color = [bg_r, bg_g, bg_b, bg_a];
        let cursor_color = [cursor_r, cursor_g, cursor_b, cursor_a];
        surface.renderer.render(&mut instance.terminal, bg_color, cursor_color, viewport_offset)
    }));

    match result {
        Ok(Ok(())) => true,
        Ok(Err(_)) | Err(_) => false,
    }
}

/// Resize the GPU rendering surface.
///
/// # Safety
/// `surface` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_surface_resize(
    surface: *mut WezTermSurface,
    width: u32,
    height: u32,
    scale_factor: f64,
) {
    if surface.is_null() {
        return;
    }
    (*surface).renderer.resize(width, height, scale_factor);
}

/// Update the surface font and DPI-derived metrics.
///
/// # Safety
/// `surface` must be valid. `font_family` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn wezterm_surface_set_font(
    surface: *mut WezTermSurface,
    font_family: *const c_char,
    font_size: f64,
    scale_factor: f64,
) -> bool {
    if surface.is_null() {
        return false;
    }

    let family = if font_family.is_null() {
        None
    } else {
        match CStr::from_ptr(font_family).to_str() {
            Ok(family) if !family.is_empty() => Some(family),
            _ => None,
        }
    };

    let dpi = (scale_factor * 72.0) as usize;
    match font_config::make_font_config(dpi, family, Some(font_size)) {
        Ok((font_config, config)) => {
            let surface = &mut *surface;
            surface.config = config;
            surface.font_config = Rc::clone(&font_config);
            surface.renderer.set_font_config(font_config).is_ok()
        }
        Err(_) => false,
    }
}

/// Get the cell dimensions (in pixels) for the current font.
///
/// # Safety
/// `surface`, `out_width`, and `out_height` must be valid.
#[no_mangle]
pub unsafe extern "C" fn wezterm_surface_get_cell_size(
    surface: *const WezTermSurface,
    out_width: *mut f64,
    out_height: *mut f64,
) {
    if surface.is_null() {
        return;
    }
    let (cw, ch) = (*surface).renderer.cell_size();
    if !out_width.is_null() {
        *out_width = cw;
    }
    if !out_height.is_null() {
        *out_height = ch;
    }
}
