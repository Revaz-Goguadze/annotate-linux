//! Debug/perf toggles read from the environment.

/// True when `name` is set to exactly `"1"`.
pub fn flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1")
}
