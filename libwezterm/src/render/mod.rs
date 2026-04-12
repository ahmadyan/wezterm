//! GPU rendering module for libwezterm.
//!
//! Uses wezterm-gui's actual rendering pipeline (GlyphCache, RenderState,
//! etc.) — not a custom reimplementation. See `gpu.rs` for the embedding glue
//! that wires our libwezterm Terminal handle into wezterm-gui's renderer.

pub mod gpu;
