//! Font configuration adapter for the FFI surface.

use config::{ConfigHandle, FontAttributes, TextStyle};
use std::rc::Rc;
use wezterm_font::FontConfiguration;

/// Create a FontConfiguration using WezTerm's defaults, optionally applying
/// the caller's preferred font family and size before the metrics are derived.
pub fn make_font_config(
    dpi: usize,
    font_family: Option<&str>,
    font_size: Option<f64>,
) -> anyhow::Result<(Rc<FontConfiguration>, ConfigHandle)> {
    let mut config = (*config::configuration()).clone();

    if let Some(font_family) = font_family.filter(|family| !family.trim().is_empty()) {
        let mut families = vec![FontAttributes::new(font_family)];
        for fallback in [
            "JetBrainsMono Nerd Font Mono",
            "JetBrainsMono Nerd Font",
            "FiraCode Nerd Font Mono",
            "FiraCode Nerd Font",
            "Hack Nerd Font Mono",
            "Hack Nerd Font",
            "Symbols Nerd Font Mono",
            "Symbols Nerd Font",
            "Apple Symbols",
            "Menlo",
        ] {
            if fallback != font_family {
                families.push(FontAttributes::new_fallback(fallback));
            }
        }

        config.font = TextStyle {
            font: families,
            foreground: None,
        };
        config.font_rules.clear();
    }

    if let Some(font_size) = font_size.filter(|size| *size > 0.0) {
        config.font_size = font_size;
    }

    config.compute_extra_defaults(None);
    config::use_this_configuration(config);

    let config = config::configuration();
    let font_config = Rc::new(FontConfiguration::new(Some(config.clone()), dpi)?);
    Ok((font_config, config))
}
