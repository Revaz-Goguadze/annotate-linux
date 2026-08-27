//! String-tagged enums (tools, board kinds, cursor styles) all need the
//! same `name()` / `from_name()` pair for config, IPC and state files.

/// Generate `name()` and `from_name()` for a unit-variant enum.
///
/// The first spelling of a variant is canonical — what `name()` returns and
/// what gets written to config/state files; the rest are accepted aliases.
///
/// ```ignore
/// named_enum!(Tool {
///     Tool::Pen => "pen",
///     Tool::Rect => "rect" | "rectangle",
/// });
/// ```
#[macro_export]
macro_rules! named_enum {
    ($ty:ty { $($variant:path => $canonical:literal $(| $alias:literal)* ),+ $(,)? }) => {
        impl $ty {
            /// The canonical name, stable across config, IPC and state files.
            pub fn name(self) -> &'static str {
                match self {
                    $($variant => $canonical,)+
                }
            }

            /// Parse a canonical name or one of its aliases.
            pub fn from_name(s: &str) -> Option<Self> {
                Some(match s {
                    $($canonical $(| $alias)* => $variant,)+
                    _ => return None,
                })
            }
        }
    };
}
