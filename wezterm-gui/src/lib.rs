//! Library entry point for wezterm-gui.
//!
//! This exposes a subset of wezterm-gui's modules so that embedders (such
//! as libwezterm) can use the actual rendering pipeline (glyphcache,
//! renderstate, screen_line, etc.) verbatim rather than reimplementing it.
//!
//! Only modules that are needed for embedded rendering are exposed here.
//! Modules that depend on the full TermWindow / mux / event loop architecture
//! (frontend, spawn, scripting, overlay, update, ...) are intentionally not
//! re-exported.

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(clippy::all)]

// Mirror the glob imports from main.rs so that `crate::color`, `crate::Dimensions`,
// etc. resolve from within the rendering modules (which use absolute paths).
use ::window::*;
use mux::activity::Activity;
use mux::Mux;

// Re-exports needed by termwindow modules
pub use ::window::color;

use config::ConfigHandle;
use std::rc::Rc;
use wezterm_font::FontConfiguration;

// cell_pixel_dims is referenced from termwindow/spawn.rs and elsewhere via
// `crate::cell_pixel_dims`. Mirror the implementation from main.rs.
pub fn cell_pixel_dims(config: &ConfigHandle, dpi: f64) -> anyhow::Result<(usize, usize)> {
    let fontconfig = Rc::new(FontConfiguration::new(Some(config.clone()), dpi as usize)?);
    let render_metrics = utilsprites::RenderMetrics::new(&fontconfig)?;
    Ok((
        render_metrics.cell_size.width as usize,
        render_metrics.cell_size.height as usize,
    ))
}

pub mod colorease;
pub mod customglyph;
pub mod glyphcache;
pub mod quad;
pub mod renderstate;
pub mod scrollbar;
pub mod selection;
pub mod shapecache;
pub mod stats;
pub mod tabbar;
pub mod termwindow;
pub mod unicode_names;
pub mod uniforms;
pub mod utilsprites;

// Modules referenced by termwindow but not exposed publicly. We still need
// them as siblings so `crate::commands::...` etc. resolve from within the
// rendering modules' source files (which use `crate::` paths).
pub mod commands;
pub mod download;
pub mod frontend;
pub mod inputmap;
pub mod overlay;
pub mod resize_increment_calculator;
pub mod scripting;
pub mod spawn;
pub mod update;

pub use selection::SelectionMode;
pub use termwindow::{set_window_class, set_window_position, TermWindow, ICON_DATA};
