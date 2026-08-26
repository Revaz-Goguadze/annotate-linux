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

    /// Parse `#rrggbb` or `#rrggbbaa`.
    pub fn parse(s: &str) -> Result<Self> {
        let hex = s.strip_prefix('#').unwrap_or(s);
        let byte = |i: usize| -> Result<f64> {
            Ok(u8::from_str_radix(&hex[i..i + 2], 16).map(|v| v as f64 / 255.0)?)
        };
        match hex.len() {
            6 => Ok(Self::new(byte(0)?, byte(2)?, byte(4)?, 1.0)),
            8 => Ok(Self::new(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => bail!("bad color {s:?}: expected #rrggbb or #rrggbbaa"),
        }
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
}
