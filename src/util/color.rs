use anyhow::{Result, bail};

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Rgba {
    pub const fn new(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self { r, g, b, a }
    }

    /// Parse `#rrggbb` or `#rrggbbaa`. ASCII hex digits only: length and
    /// alphabet are checked before any indexing, so multi-byte input can
    /// never slice mid-character.
    pub fn parse(s: &str) -> Result<Self> {
        let hex = s.strip_prefix('#').unwrap_or(s).as_bytes();
        if !matches!(hex.len(), 6 | 8) || !hex.iter().all(|b| b.is_ascii_hexdigit()) {
            bail!("bad color {s:?}: expected #rrggbb or #rrggbbaa");
        }
        let byte = |i: usize| -> f64 {
            let nibble = |b: u8| f64::from((b as char).to_digit(16).unwrap_or(0));
            (nibble(hex[i]) * 16.0 + nibble(hex[i + 1])) / 255.0
        };
        Ok(if hex.len() == 6 {
            Self::new(byte(0), byte(2), byte(4), 1.0)
        } else {
            Self::new(byte(0), byte(2), byte(4), byte(6))
        })
    }

    pub fn to_hex(self) -> String {
        format!(
            "#{:02x}{:02x}{:02x}",
            (self.r * 255.0).round() as u8,
            (self.g * 255.0).round() as u8,
            (self.b * 255.0).round() as u8
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_back() {
        let c = Rgba::parse("#e53935").unwrap();
        assert_eq!(c.to_hex(), "#e53935");
        assert_eq!(c.a, 1.0);
        let c = Rgba::parse("ff000080").unwrap();
        assert_eq!((c.r, c.g, c.b), (1.0, 0.0, 0.0));
        assert!((c.a - 128.0 / 255.0).abs() < 1e-9);
        assert!(Rgba::parse("#12345").is_err());
    }

    #[test]
    fn non_hex_input_is_rejected_not_panicked() {
        // Six-byte, three-char strings used to slice mid-character.
        assert!(Rgba::parse("€00000").is_err());
        assert!(Rgba::parse("#€00000").is_err());
        assert!(Rgba::parse("zzzzzz").is_err());
        assert!(Rgba::parse("#ff 000").is_err());
    }
}
