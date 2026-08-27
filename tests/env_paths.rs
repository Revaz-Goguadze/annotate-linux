//! Path resolution and state persistence depend on process-wide environment
//! variables, so they live in their own test binary and are serialized here.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

use annotate_linux::config::state::RuntimeState;
use annotate_linux::ipc;
use annotate_linux::util::xdg;

const VARS: [&str; 5] =
    ["HOME", "XDG_CONFIG_HOME", "XDG_STATE_HOME", "XDG_RUNTIME_DIR", "WAYLAND_DISPLAY"];

/// Held for the duration of a test: serializes access to the environment and
/// restores whatever the harness started with.
struct EnvGuard {
    _lock: MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn new() -> Self {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let lock = LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|e| e.into_inner());
        let saved = VARS.iter().map(|k| (*k, std::env::var(k).ok())).collect();
        let mut guard = Self { _lock: lock, saved };
        for key in VARS {
            guard.unset(key);
        }
        guard
    }

    fn set(&mut self, key: &str, value: impl AsRef<std::ffi::OsStr>) {
        unsafe { std::env::set_var(key, value) };
    }

    fn unset(&mut self, key: &str) {
        unsafe { std::env::remove_var(key) };
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in std::mem::take(&mut self.saved) {
            match value {
                Some(v) => unsafe { std::env::set_var(key, v) },
                None => unsafe { std::env::remove_var(key) },
            }
        }
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("annotate-env-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn config_and_state_dirs_follow_the_xdg_overrides() {
    let mut env = EnvGuard::new();
    env.set("HOME", "/home/tester");
    env.set("XDG_CONFIG_HOME", "/cfg");
    env.set("XDG_STATE_HOME", "/st");
    assert_eq!(xdg::config_dir(), PathBuf::from("/cfg/annotate-linux"));
    assert_eq!(xdg::state_dir(), PathBuf::from("/st/annotate-linux"));
}

#[test]
fn config_and_state_dirs_fall_back_to_home() {
    let mut env = EnvGuard::new();
    env.set("HOME", "/home/tester");
    assert_eq!(xdg::config_dir(), PathBuf::from("/home/tester/.config/annotate-linux"));
    assert_eq!(xdg::state_dir(), PathBuf::from("/home/tester/.local/state/annotate-linux"));
}

#[test]
fn dirs_fall_back_to_the_root_without_a_home() {
    let _env = EnvGuard::new();
    assert_eq!(xdg::config_dir(), PathBuf::from("/.config/annotate-linux"));
    assert_eq!(xdg::state_dir(), PathBuf::from("/.local/state/annotate-linux"));
}

#[test]
fn socket_path_is_keyed_by_the_wayland_display() {
    let mut env = EnvGuard::new();
    env.set("XDG_RUNTIME_DIR", "/run/user/1000");
    env.set("WAYLAND_DISPLAY", "wayland-1");
    assert_eq!(
        ipc::socket_path().unwrap(),
        PathBuf::from("/run/user/1000/annotate-linux/wayland-1.sock")
    );
}

#[test]
fn socket_path_names_the_missing_variable() {
    let mut env = EnvGuard::new();
    let err = ipc::socket_path().unwrap_err().to_string();
    assert!(err.contains("XDG_RUNTIME_DIR"), "{err}");

    env.set("XDG_RUNTIME_DIR", "/run/user/1000");
    let err = ipc::socket_path().unwrap_err().to_string();
    assert!(err.contains("WAYLAND_DISPLAY"), "{err}");
}

#[test]
fn client_send_reports_the_socket_it_could_not_reach() {
    let mut env = EnvGuard::new();
    let dir = scratch("no-daemon");
    env.set("XDG_RUNTIME_DIR", &dir);
    env.set("WAYLAND_DISPLAY", "wayland-test");
    let err = ipc::client::send(&ipc::protocol::Command::Status).unwrap_err().to_string();
    assert!(err.contains("no daemon running"), "{err}");
    assert!(err.contains("wayland-test.sock"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn runtime_state_saves_atomically_and_loads_back() {
    let mut env = EnvGuard::new();
    let dir = scratch("state");
    env.set("XDG_STATE_HOME", &dir);

    assert_eq!(RuntimeState::load(), RuntimeState::default(), "missing file yields defaults");

    let saved = RuntimeState {
        tool: "highlighter".into(),
        color: "#ff0000".into(),
        width: 9.5,
        board: "black".into(),
        fade: true,
    };
    saved.save().unwrap();

    let state_file = dir.join("annotate-linux/state.toml");
    assert!(state_file.is_file(), "save() must create {}", state_file.display());
    assert!(!dir.join("annotate-linux/state.toml.tmp").exists(), "temp file must be renamed away");
    assert_eq!(RuntimeState::load(), saved);

    std::fs::write(&state_file, "width = \"wide\"\n").unwrap();
    assert_eq!(RuntimeState::load(), RuntimeState::default(), "corrupt file yields defaults");

    std::fs::remove_file(&state_file).unwrap();
    std::fs::create_dir(&state_file).unwrap();
    assert_eq!(RuntimeState::load(), RuntimeState::default(), "unreadable file yields defaults");

    let _ = std::fs::remove_dir_all(&dir);
}
