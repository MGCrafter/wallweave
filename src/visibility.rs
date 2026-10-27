//! Hyprland workspace-aware pause detection. Foreign-toplevel state alone can
//! retain focus/fullscreen flags after a special workspace has been hidden.
use crate::config::AutoPause;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const INTERVAL: Duration = Duration::from_millis(250);
const MAX_AGE: Duration = Duration::from_secs(2);
const MAX_REPLY: u64 = 1024 * 1024;

pub fn socket_path() -> Option<PathBuf> {
    let signature = env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    if signature.is_empty() || signature.contains('/') || signature == ".." {
        return None;
    }
    Some(
        PathBuf::from(env::var_os("XDG_RUNTIME_DIR")?)
            .join("hypr")
            .join(signature)
            .join(".socket.sock"),
    )
}

#[derive(Clone, Copy, Debug, Default)]
struct OutputState {
    focused: bool,
    fullscreen: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    outputs: BTreeMap<String, OutputState>,
    game_running: bool,
}

impl Snapshot {
    pub fn game_running(&self) -> bool {
        self.game_running
    }
    pub fn connected(&self, output: &str) -> bool {
        self.outputs.contains_key(output)
    }
    pub fn pauses(&self, output: &str, mode: AutoPause) -> bool {
        self.outputs.get(output).is_some_and(|state| match mode {
            AutoPause::Off => false,
            AutoPause::Fullscreen => state.fullscreen,
            AutoPause::Focused => state.focused || state.fullscreen,
        })
    }
}

#[derive(Deserialize)]
struct Workspace {
    id: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Monitor {
    id: i64,
    name: String,
    active_workspace: Workspace,
    special_workspace: Option<Workspace>,
    #[serde(default)]
    disabled: bool,
}

#[derive(Deserialize)]
struct Window {
    address: String,
    monitor: i64,
    workspace: Workspace,
    mapped: bool,
    hidden: bool,
    visible: Option<bool>,
    #[serde(default)]
    pinned: bool,
    fullscreen: u8,
    #[serde(default)]
    class: String,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct ActiveWindow {
    #[serde(default)]
    address: String,
}

fn parse(monitors: &[u8], clients: &[u8], active: &[u8]) -> Result<Snapshot> {
    let monitors: Vec<Monitor> = serde_json::from_slice(monitors).context("Hyprland monitors")?;
    let clients: Vec<Window> = serde_json::from_slice(clients).context("Hyprland clients")?;
    let active: ActiveWindow = serde_json::from_slice(active).context("Hyprland active window")?;
    let game_running = clients.iter().any(|w| {
        w.mapped
            && (w.class == "gamescope"
                || w.class
                    .strip_prefix("steam_app_")
                    .is_some_and(|id| id.parse::<u64>().is_ok_and(|id| id != 0 && id != 431960))
                || w.tags.iter().any(|t| t.trim_end_matches('*') == "game"))
    });
    let mut snapshot = Snapshot {
        game_running,
        ..Default::default()
    };
    for monitor in monitors.iter().filter(|m| !m.disabled) {
        let mut state = OutputState::default();
        for window in clients.iter().filter(|w| w.monitor == monitor.id) {
            if !window.mapped || window.hidden || window.visible == Some(false) {
                continue;
            }
            // `mapped`, `hidden` and even `visible` can remain unchanged when
            // special workspaces close. Membership in a visible workspace is
            // required even if activewindow still points to the hidden client.
            let visible_workspace = if window.workspace.id < 0 {
                monitor
                    .special_workspace
                    .as_ref()
                    .is_some_and(|s| s.id == window.workspace.id)
            } else {
                window.pinned || window.workspace.id == monitor.active_workspace.id
            };
            if !visible_workspace {
                continue;
            }
            state.focused |= !active.address.is_empty() && window.address == active.address;
            // Hyprland's internal state: 1=maximized, 2=fullscreen, 3=both.
            // fullscreenClient is deliberately ignored (client-only fullscreen).
            state.fullscreen |= window.fullscreen & 3 != 0;
        }
        snapshot.outputs.insert(monitor.name.clone(), state);
    }
    Ok(snapshot)
}

fn query(path: &Path, command: &str) -> Result<Vec<u8>> {
    let mut socket = UnixStream::connect(path).context("Connect to Hyprland")?;
    socket.set_read_timeout(Some(Duration::from_millis(200)))?;
    socket.set_write_timeout(Some(Duration::from_millis(200)))?;
    socket.write_all(command.as_bytes())?;
    let mut bytes = Vec::new();
    socket.take(MAX_REPLY + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_REPLY,
        "Hyprland reply exceeds 1 MiB"
    );
    Ok(bytes)
}

fn read(path: &Path) -> Result<Snapshot> {
    parse(
        &query(path, "j/monitors")?,
        &query(path, "j/clients")?,
        &query(path, "j/activewindow")?,
    )
}

type Observation = Option<(Instant, std::result::Result<Snapshot, String>)>;

pub struct Watcher {
    latest: Arc<Mutex<Observation>>,
    enabled: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
}

impl Watcher {
    pub fn start() -> Option<Self> {
        Self::at(socket_path()?).ok()
    }

    fn at(path: PathBuf) -> Result<Self> {
        let watcher = Self {
            latest: Arc::new(Mutex::new(None)),
            enabled: Arc::new(AtomicBool::new(false)),
            alive: Arc::new(AtomicBool::new(true)),
        };
        let latest = watcher.latest.clone();
        let enabled = watcher.enabled.clone();
        let alive = watcher.alive.clone();
        thread::Builder::new()
            .name("workspace-pause".into())
            .spawn(move || {
                while alive.load(Ordering::Relaxed) {
                    let observation = enabled.load(Ordering::Relaxed).then(|| {
                        let observed_at = Instant::now();
                        let value = read(&path).map_err(|e| format!("{e:#}"));
                        (observed_at, value)
                    });
                    if let Ok(mut current) = latest.lock() {
                        *current = observation;
                    }
                    thread::sleep(INTERVAL);
                }
            })?;
        Ok(watcher)
    }

    pub fn enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// Fail open on broken/stale IPC. Never leave a wallpaper frozen because
    /// the compositor stopped answering. The supervisor and GUI never do IO here.
    pub fn snapshot(&self) -> std::result::Result<Snapshot, String> {
        let latest = self
            .latest
            .lock()
            .map_err(|_| "Hyprland watcher unavailable")?;
        match latest.as_ref() {
            Some((at, value)) if at.elapsed() <= MAX_AGE => value.clone(),
            Some(_) => Err("Hyprland window information expired".into()),
            None => Ok(Snapshot::default()),
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        // No join: even a blocked local socket connect must not block shutdown.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn snapshot(monitors: Value, windows: Value, active: &str) -> Snapshot {
        parse(
            &serde_json::to_vec(&monitors).expect("monitors"),
            &serde_json::to_vec(&windows).expect("clients"),
            &serde_json::to_vec(&json!({"address":active})).expect("active"),
        )
        .expect("snapshot")
    }

    fn monitor(special: i64) -> Value {
        json!([{"id":1,"name":"DP-1","activeWorkspace":{"id":13},"specialWorkspace":{"id":special}}])
    }

    fn window(workspace: i64) -> Value {
        json!({"address":"0x1","monitor":1,"workspace":{"id":workspace},"mapped":true,"hidden":false,"visible":true,"fullscreen":0})
    }

    #[test]
    fn games_remain_detected_when_hidden_or_on_another_workspace() {
        for class in ["steam_app_1422450", "gamescope", "custom-game"] {
            let mut client = window(-96);
            client["class"] = json!(class);
            client["hidden"] = json!(true);
            client["visible"] = json!(false);
            if class == "custom-game" {
                client["tags"] = json!(["game*"]);
            }
            let state = snapshot(monitor(0), json!([client]), "other-app");
            assert!(state.game_running());
            assert!(!state.pauses("DP-1", AutoPause::Focused));
            assert!(state.connected("DP-1"));
            assert!(!state.connected("DP-2"));
            client["mapped"] = json!(false);
            assert!(!snapshot(monitor(0), json!([client]), "").game_running());
        }
        for class in [
            "steam",
            "steam_app_431960",
            "steam_app_0",
            "steam_app_invalid",
            "firefox",
        ] {
            let mut client = window(13);
            client["class"] = json!(class);
            client["fullscreen"] = json!(2);
            assert!(!snapshot(monitor(0), json!([client]), "0x1").game_running());
        }
    }

    #[test]
    fn hidden_special_workspace_ignores_stale_focus_and_fullscreen() {
        for mode in [0, 1, 2, 3] {
            let mut client = window(-96);
            client["fullscreen"] = json!(mode);
            let open = snapshot(monitor(-96), json!([client]), "0x1");
            assert!(open.pauses("DP-1", AutoPause::Focused));
            assert_eq!(open.pauses("DP-1", AutoPause::Fullscreen), mode != 0);
            let closed = snapshot(monitor(0), json!([client]), "0x1");
            assert!(!closed.pauses("DP-1", AutoPause::Focused));
            assert!(!closed.pauses("DP-1", AutoPause::Fullscreen));
            assert!(
                snapshot(monitor(-96), json!([client]), "0x1").pauses("DP-1", AutoPause::Focused)
            );
        }
    }

    #[test]
    fn closed_windows_inactive_workspaces_and_hidden_group_members_resume() {
        assert!(!snapshot(monitor(0), json!([]), "0x1").pauses("DP-1", AutoPause::Focused));
        assert!(
            !snapshot(monitor(0), json!([window(12)]), "0x1").pauses("DP-1", AutoPause::Focused)
        );
        for (field, value) in [("mapped", false), ("hidden", true), ("visible", false)] {
            let mut client = window(13);
            client[field] = json!(value);
            assert!(
                !snapshot(monitor(0), json!([client]), "0x1").pauses("DP-1", AutoPause::Focused)
            );
        }
        let mut pinned = window(12);
        pinned["pinned"] = json!(true);
        let current = snapshot(monitor(0), json!([pinned]), "0x1");
        assert!(current.pauses("DP-1", AutoPause::Focused));
        assert!(!current.pauses("DP-2", AutoPause::Focused));
        assert!(!current.pauses("DP-1", AutoPause::Off));
        assert!(!current.pauses("DP-1", AutoPause::Fullscreen));
    }

    #[test]
    fn missing_focus_and_client_only_fullscreen_do_not_pause() {
        let mut client = window(13);
        client["fullscreenClient"] = json!(2);
        let current = snapshot(monitor(0), json!([client]), "");
        assert!(!current.pauses("DP-1", AutoPause::Focused));
        assert!(!current.pauses("DP-1", AutoPause::Fullscreen));
        assert!(parse(b"{}", b"[]", b"{}").is_err());
    }

    #[test]
    fn stale_observation_cannot_hold_an_automatic_pause() {
        let snapshot = snapshot(monitor(-96), json!([window(-96)]), "0x1");
        let watcher = Watcher {
            latest: Arc::new(Mutex::new(Some((
                Instant::now() - MAX_AGE - INTERVAL,
                Ok(snapshot),
            )))),
            enabled: Arc::new(AtomicBool::new(true)),
            alive: Arc::new(AtomicBool::new(true)),
        };
        assert!(watcher.snapshot().is_err());
        assert!(
            !watcher
                .snapshot()
                .unwrap_or_default()
                .pauses("DP-1", AutoPause::Focused)
        );
    }
}
