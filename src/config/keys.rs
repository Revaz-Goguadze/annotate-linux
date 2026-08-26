//! Rebindable keys: a `[keys]` table of `"ctrl+shift+z" = "redo"` entries
//! layered over the defaults. Validation errors name the offending entry.

use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};
use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::input::{Action, Tool};
use crate::model::constraints::Mods;

const MOD_CTRL: u8 = 1;
const MOD_ALT: u8 = 2;
const MOD_SHIFT: u8 = 4;
const MOD_LOGO: u8 = 8;

#[derive(Clone, Debug)]
pub struct Keymap {
    map: HashMap<(u32, u8), Action>,
}

fn mods_bits(m: Mods) -> u8 {
    (m.ctrl as u8) * MOD_CTRL + (m.alt as u8) * MOD_ALT + (m.shift as u8) * MOD_SHIFT + (m.logo as u8) * MOD_LOGO
}

/// Lowercase latin keysyms so `Shift+Z` (keysym `Z`) matches a `z` binding.
fn normalize(sym: u32) -> u32 {
    if (0x41..=0x5a).contains(&sym) { sym + 0x20 } else { sym }
}

fn keysym_from_name(name: &str) -> Result<u32> {
    let sym = match name.to_ascii_lowercase().as_str() {
        "escape" | "esc" => Keysym::Escape.raw(),
        "delete" | "del" => Keysym::Delete.raw(),
        "backspace" => Keysym::BackSpace.raw(),
        "return" | "enter" => Keysym::Return.raw(),
        "space" => Keysym::space.raw(),
        "tab" => Keysym::Tab.raw(),
        "up" => Keysym::Up.raw(),
        "down" => Keysym::Down.raw(),
        "left" => Keysym::Left.raw(),
        "right" => Keysym::Right.raw(),
        s if s.chars().count() == 1 && s.is_ascii() => {
            let c = s.chars().next().expect("one char");
            if !c.is_ascii_graphic() {
                bail!("unsupported key {name:?}");
            }
            c.to_ascii_lowercase() as u32
        }
        _ => bail!("unknown key name {name:?}"),
    };
    Ok(sym)
}

fn parse_keyspec(spec: &str) -> Result<(u32, u8)> {
    let mut bits = 0u8;
    let mut key: Option<&str> = None;
    for part in spec.split('+') {
        match part.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" => bits |= MOD_CTRL,
            "alt" => bits |= MOD_ALT,
            "shift" => bits |= MOD_SHIFT,
            "logo" | "super" | "mod4" => bits |= MOD_LOGO,
            "" => bail!("empty component in keyspec {spec:?}"),
            _ => {
                if key.is_some() {
                    bail!("more than one key in keyspec {spec:?}");
                }
                key = Some(part.trim());
            }
        }
    }
    let name = key.ok_or_else(|| anyhow!("no key in keyspec {spec:?}"))?;
    Ok((keysym_from_name(name)?, bits))
}

pub fn parse_action(s: &str) -> Result<Action> {
    if let Some(tool) = s.strip_prefix("tool:") {
        return Tool::from_name(tool)
            .map(Action::SelectTool)
            .ok_or_else(|| anyhow!("unknown tool {tool:?}"));
    }
    Ok(match s {
        "undo" => Action::Undo,
        "redo" => Action::Redo,
        "clear" => Action::Clear,
        "hide" => Action::Hide,
        "color-picker" => Action::ToggleColorPicker,
        "width-picker" => Action::ToggleWidthPicker,
        "board" => Action::CycleBoard,
        "counter-reset" => Action::CounterReset,
        "copy" => Action::Copy,
        "cut" => Action::Cut,
        "paste" => Action::Paste,
        "duplicate" => Action::Duplicate,
        "delete" => Action::DeleteSelection,
        _ => bail!("unknown action {s:?}"),
    })
}

impl Keymap {
    /// The built-in bindings (matching the documented single-key defaults).
    pub fn defaults() -> Self {
        let table: &[(&str, &str)] = &[
            ("p", "tool:pen"),
            ("h", "tool:highlighter"),
            ("l", "tool:line"),
            ("a", "tool:arrow"),
            ("r", "tool:rect"),
            ("e", "tool:ellipse"),
            ("n", "tool:counter"),
            ("t", "tool:text"),
            ("s", "tool:select"),
            ("x", "tool:eraser"),
            ("c", "color-picker"),
            ("w", "width-picker"),
            ("b", "board"),
            ("escape", "hide"),
            ("delete", "delete"),
            ("ctrl+z", "undo"),
            ("ctrl+shift+z", "redo"),
            ("ctrl+r", "counter-reset"),
            ("ctrl+c", "copy"),
            ("ctrl+x", "cut"),
            ("ctrl+v", "paste"),
            ("ctrl+d", "duplicate"),
        ];
        let mut map = HashMap::new();
        for (spec, action) in table {
            let k = parse_keyspec(spec).expect("default keyspec");
            map.insert(k, parse_action(action).expect("default action"));
        }
        Self { map }
    }

    /// Defaults overlaid with the user `[keys]` table. Errors name the bad
    /// entry; an empty action string unbinds the key.
    pub fn with_overrides(user: &HashMap<String, String>) -> Result<Self> {
        let mut km = Self::defaults();
        for (spec, action) in user {
            let k = parse_keyspec(spec).map_err(|e| anyhow!("[keys] {spec:?}: {e}"))?;
            if action.is_empty() {
                km.map.remove(&k);
            } else {
                let a = parse_action(action).map_err(|e| anyhow!("[keys] {spec:?} = {action:?}: {e}"))?;
                km.map.insert(k, a);
            }
        }
        // Esc stays a guaranteed escape hatch.
        km.map.insert(
            (Keysym::Escape.raw(), 0),
            Action::Hide,
        );
        Ok(km)
    }

    /// Look up by the layout-produced keysym first; on a miss, retry with
    /// the US-layout letter for the physical key (`raw_code`, evdev) so
    /// single-key shortcuts keep working under non-Latin layouts (e.g. a
    /// `us,ge` setup with Georgian active). Text input is unaffected — it
    /// reads utf8, not this table.
    pub fn lookup(&self, keysym: Keysym, raw_code: u32, mods: Mods) -> Option<Action> {
        let bits = mods_bits(mods);
        if let Some(a) = self.map.get(&(normalize(keysym.raw()), bits)) {
            return Some(a.clone());
        }
        let fallback = ascii_for_evdev(raw_code)?;
        self.map.get(&(fallback, bits)).cloned()
    }
}

/// evdev keycode → lowercase ASCII keysym (US layout positions).
pub fn ascii_for_evdev(code: u32) -> Option<u32> {
    let c: u8 = match code {
        16 => b'q', 17 => b'w', 18 => b'e', 19 => b'r', 20 => b't',
        21 => b'y', 22 => b'u', 23 => b'i', 24 => b'o', 25 => b'p',
        30 => b'a', 31 => b's', 32 => b'd', 33 => b'f', 34 => b'g',
        35 => b'h', 36 => b'j', 37 => b'k', 38 => b'l',
        44 => b'z', 45 => b'x', 46 => b'c', 47 => b'v', 48 => b'b',
        49 => b'n', 50 => b'm',
        2..=10 => b'1' + (code as u8 - 2),
        11 => b'0',
        _ => return None,
    };
    Some(c as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Mods = Mods { shift: false, ctrl: false, alt: false, logo: false };
    const CTRL: Mods = Mods { shift: false, ctrl: true, alt: false, logo: false };
    const CTRL_SHIFT: Mods = Mods { shift: true, ctrl: true, alt: false, logo: false };

    /// evdev code that maps to no ASCII fallback.
    const NO_KEY: u32 = 999;

    #[test]
    fn defaults_cover_documented_keys() {
        let km = Keymap::defaults();
        assert_eq!(km.lookup(Keysym::p, NO_KEY, NONE), Some(Action::SelectTool(Tool::Pen)));
        assert_eq!(km.lookup(Keysym::z, NO_KEY, CTRL), Some(Action::Undo));
        // Shift+z arrives as uppercase keysym Z — must still find redo
        assert_eq!(km.lookup(Keysym::Z, NO_KEY, CTRL_SHIFT), Some(Action::Redo));
        assert_eq!(km.lookup(Keysym::Escape, NO_KEY, NONE), Some(Action::Hide));
        assert_eq!(km.lookup(Keysym::q, NO_KEY, NONE), None);
    }

    #[test]
    fn non_latin_layout_falls_back_to_physical_key() {
        let km = Keymap::defaults();
        // Georgian 'პ' keysym on the physical P key (evdev 25) → pen
        let georgian_p = Keysym::new(0x100_10de_u32 + 0x35); // arbitrary non-matching sym
        assert_eq!(km.lookup(georgian_p, 25, NONE), Some(Action::SelectTool(Tool::Pen)));
        // Ctrl+z on physical Z key (evdev 44) under any layout → undo
        assert_eq!(km.lookup(georgian_p, 44, CTRL), Some(Action::Undo));
        // keysym match always wins over the physical position
        assert_eq!(km.lookup(Keysym::h, 25, NONE), Some(Action::SelectTool(Tool::Highlighter)));
    }

    #[test]
    fn override_rebinds_and_unbinds() {
        let mut user = HashMap::new();
        user.insert("q".to_string(), "tool:arrow".to_string());
        user.insert("p".to_string(), String::new()); // unbind pen
        let km = Keymap::with_overrides(&user).unwrap();
        assert_eq!(km.lookup(Keysym::q, NO_KEY, NONE), Some(Action::SelectTool(Tool::Arrow)));
        assert_eq!(km.lookup(Keysym::p, NO_KEY, NONE), None);
        // unbound keysym, but physical P still resolves via fallback? No —
        // the fallback maps to the same (unbound) entry, so it stays dead.
        assert_eq!(km.lookup(Keysym::p, 25, NONE), None);
    }

    #[test]
    fn bad_entries_name_the_key() {
        let mut user = HashMap::new();
        user.insert("ctrl+meh".to_string(), "undo".to_string());
        let err = Keymap::with_overrides(&user).unwrap_err().to_string();
        assert!(err.contains("ctrl+meh"), "{err}");

        let mut user = HashMap::new();
        user.insert("q".to_string(), "warp-speed".to_string());
        let err = Keymap::with_overrides(&user).unwrap_err().to_string();
        assert!(err.contains("warp-speed"), "{err}");
    }

    #[test]
    fn esc_hide_cannot_be_unbound() {
        let mut user = HashMap::new();
        user.insert("escape".to_string(), String::new());
        let km = Keymap::with_overrides(&user).unwrap();
        assert_eq!(km.lookup(Keysym::Escape, NO_KEY, NONE), Some(Action::Hide));
    }
}
