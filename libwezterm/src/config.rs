//! FFI-compatible implementation of the TerminalConfiguration trait.

use crate::types::WezTermConfig;
use wezterm_term::color::ColorPalette;
use wezterm_term::config::{BidiMode, NewlineCanon, TerminalConfiguration};
use wezterm_bidi::ParagraphDirectionHint;
use wezterm_cell::UnicodeVersion;

/// Terminal configuration backed by FFI-provided values.
#[derive(Debug)]
pub struct FFITerminalConfig {
    scrollback_size: usize,
    palette: ColorPalette,
    enable_kitty_graphics: bool,
    enable_kitty_keyboard: bool,
}

impl Default for FFITerminalConfig {
    fn default() -> Self {
        Self {
            scrollback_size: 3500,
            palette: ColorPalette::default(),
            enable_kitty_graphics: false,
            enable_kitty_keyboard: true,
        }
    }
}

impl FFITerminalConfig {
    /// Create a config from the C FFI struct.
    ///
    /// # Safety
    /// The `cfg` reference must be valid, and `cfg.ansi_colors` (if non-null)
    /// must point to at least `cfg.ansi_color_count` entries.
    pub unsafe fn from_ffi(cfg: &WezTermConfig) -> Self {
        let mut palette = ColorPalette::default();

        palette.foreground = cfg.foreground.to_srgba();
        palette.background = cfg.background.to_srgba();
        palette.cursor_fg = cfg.cursor_fg.to_srgba();
        palette.cursor_bg = cfg.cursor_bg.to_srgba();

        if !cfg.ansi_colors.is_null() && cfg.ansi_color_count > 0 {
            let count = (cfg.ansi_color_count as usize).min(256);
            for i in 0..count {
                let c = &*cfg.ansi_colors.add(i);
                palette.colors.0[i] = c.to_srgba();
            }
        }

        let scrollback = if cfg.scrollback_size == 0 {
            3500
        } else {
            cfg.scrollback_size as usize
        };

        Self {
            scrollback_size: scrollback,
            palette,
            enable_kitty_graphics: cfg.enable_kitty_graphics,
            enable_kitty_keyboard: cfg.enable_kitty_keyboard,
        }
    }
}

impl TerminalConfiguration for FFITerminalConfig {
    fn scrollback_size(&self) -> usize {
        self.scrollback_size
    }

    fn color_palette(&self) -> ColorPalette {
        self.palette.clone()
    }

    fn enable_kitty_graphics(&self) -> bool {
        self.enable_kitty_graphics
    }

    fn enable_kitty_keyboard(&self) -> bool {
        self.enable_kitty_keyboard
    }

    fn canonicalize_pasted_newlines(&self) -> NewlineCanon {
        NewlineCanon::CarriageReturn
    }

    fn unicode_version(&self) -> UnicodeVersion {
        UnicodeVersion {
            version: 14,
            ambiguous_are_wide: false,
            cell_widths: None,
        }
    }

    fn enable_title_reporting(&self) -> bool {
        false
    }

    fn bidi_mode(&self) -> BidiMode {
        BidiMode {
            enabled: false,
            hint: ParagraphDirectionHint::LeftToRight,
        }
    }
}
