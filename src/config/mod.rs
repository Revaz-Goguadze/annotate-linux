pub mod keys;
pub mod state;

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::util::{toml_file, xdg};

/// User configuration, read from `~/.config/annotate-linux/config.toml`.
/// Every field has a default; a missing file means all defaults.
/// Unknown keys are tolerated (warn-don't-fail happens at the load call site).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub appearance: Appearance,
    pub cursor: Cursor,
    /// Keybinding overrides: `"ctrl+shift+z" = "redo"`, `"p" = ""` unbinds.
    pub keys: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct Cursor {
    /// default | none | outline | circle | crosshair
    pub style: String,
    /// Spotlight circle following the pointer
    pub highlight: bool,
    pub highlight_radius: f64,
    /// Expanding ring on click
    pub ripple: bool,
    pub ripple_ms: u64,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            style: "default".into(),
            highlight: false,
            highlight_radius: 48.0,
            ripple: false,
            ripple_ms: 450,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct General {
    /// "exclusive" | "on-demand" — layer-shell keyboard interactivity while interactive
    pub keyboard_interactivity: String,
    /// Clear all annotations when the overlay is toggled off
    pub auto_clear_on_toggle: bool,
    /// Fade mode duration in seconds
    pub fade_seconds: f64,
    /// Start in fade mode instead of persist
    pub fade_default: bool,
    /// Layer surface namespace (matches Hyprland layerrule)
    pub namespace: String,
}

impl Default for General {
    fn default() -> Self {
        Self {
            keyboard_interactivity: "exclusive".into(),
            auto_clear_on_toggle: false,
            fade_seconds: 3.0,
            fade_default: false,
            namespace: "annotate-linux".into(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct Appearance {
    /// Color palette as #rrggbb strings; first entry is the default color
    pub palette: Vec<String>,
    pub default_width: f64,
    /// Alpha applied to highlighter strokes (group-composited)
    pub highlighter_alpha: f64,
    /// Board opacity 0.1..=1.0
    pub board_opacity: f64,
    /// Text tool font size in logical px
    pub text_px: f64,
    /// Counter badge radius in logical px
    pub counter_radius: f64,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            palette: vec![
                "#e53935".into(), // red
                "#fb8c00".into(), // orange
                "#fdd835".into(), // yellow
                "#43a047".into(), // green
                "#1e88e5".into(), // blue
                "#8e24aa".into(), // purple
                "#000000".into(),
                "#ffffff".into(),
            ],
            default_width: 4.0,
            highlighter_alpha: 0.35,
            board_opacity: 0.85,
            text_px: 24.0,
            counter_radius: 16.0,
        }
    }
}

impl Config {
    /// Load from the default location; missing file → defaults.
    pub fn load() -> Result<Self> {
        Self::load_from(&xdg::config_dir().join("config.toml"))
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let Some(text) = toml_file::read_opt(path)? else {
            return Ok(Self::default());
        };
        let de = toml::Deserializer::parse(&text)?;
        let cfg: Config = serde_ignored::deserialize(de, |unknown| {
            log::warn!("config: unknown key `{unknown}` in {} (ignored)", path.display());
        })?;
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_roundtrip_through_toml() {
        let cfg = Config::default();
        let text = toml::to_string(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn partial_file_fills_defaults() {
        let cfg: Config = toml::from_str("[appearance]\ndefault_width = 8.0\n").unwrap();
        assert_eq!(cfg.appearance.default_width, 8.0);
        assert_eq!(cfg.general.keyboard_interactivity, "exclusive");
        assert_eq!(cfg.appearance.highlighter_alpha, 0.35);
    }

    #[test]
    fn missing_file_is_defaults() {
        let cfg = Config::load_from(Path::new("/nonexistent/annotate-test/config.toml")).unwrap();
        assert_eq!(cfg, Config::default());
    }

    fn write_config(name: &str, text: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("annotate-cfg-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn a_file_overrides_only_the_keys_it_sets() {
        let path = write_config(
            "override",
            r##"
[general]
fade_default = true
fade_seconds = 1.5

[appearance]
palette = ["#123456"]

[cursor]
style = "crosshair"
ripple = true

[keys]
"ctrl+shift+z" = "redo"
"p" = ""
"##,
        );
        let cfg = Config::load_from(&path).unwrap();
        assert!(cfg.general.fade_default);
        assert_eq!(cfg.general.fade_seconds, 1.5);
        assert_eq!(cfg.general.namespace, "annotate-linux", "untouched keys keep defaults");
        assert_eq!(cfg.appearance.palette, vec!["#123456"]);
        assert_eq!(cfg.appearance.text_px, 24.0);
        assert_eq!(cfg.cursor.style, "crosshair");
        assert!(cfg.cursor.ripple && !cfg.cursor.highlight);
        assert_eq!(cfg.keys.get("ctrl+shift+z").map(String::as_str), Some("redo"));
        assert_eq!(cfg.keys.get("p").map(String::as_str), Some(""), "empty value unbinds");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn unknown_keys_are_tolerated() {
        let path = write_config("unknown", "[general]\nnope = 1\n\n[nonsense]\nx = true\n");
        assert_eq!(Config::load_from(&path).unwrap(), Config::default());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn malformed_toml_and_wrong_types_are_errors() {
        for (name, text) in
            [("syntax", "[general\n"), ("types", "[appearance]\ndefault_width = \"wide\"\n")]
        {
            let path = write_config(name, text);
            assert!(Config::load_from(&path).is_err(), "{name} should not load");
            let _ = std::fs::remove_dir_all(path.parent().unwrap());
        }
    }

    #[test]
    fn a_directory_in_place_of_the_config_is_an_error() {
        let dir = std::env::temp_dir().join(format!("annotate-cfg-{}-dir", std::process::id()));
        std::fs::create_dir_all(dir.join("config.toml")).unwrap();
        assert!(Config::load_from(&dir.join("config.toml")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
