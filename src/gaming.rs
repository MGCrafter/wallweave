//! Detect Steam launches without attaching to or signalling games. Process lifetime,
//! rather than window focus, keeps wallpapers unloaded through Alt-Tab/loading screens.
use anyhow::{Context, Result};
use std::{
    fs,
    io::Read,
    os::unix::fs::MetadataExt,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

fn steam_game(environment: &[u8]) -> bool {
    environment.split(|b| *b == 0).any(|entry| {
        let Some(value) = entry
            .strip_prefix(b"SteamGameId=")
            .or_else(|| entry.strip_prefix(b"SteamAppId="))
        else {
            return false;
        };
        let Ok(value) = std::str::from_utf8(value) else {
            return false;
        };
        // Wallpaper Engine is often running its Workshop client while the desktop plays.
        value
            .parse::<u64>()
            .is_ok_and(|id| id != 0 && id != 431960 && id != 228980)
    })
}

fn scan(root: &Path, uid: u32) -> Result<bool> {
    for entry in fs::read_dir(root).context("Read running processes")? {
        let Ok(entry) = entry else { continue };
        if !entry
            .file_name()
            .as_encoded_bytes()
            .iter()
            .all(u8::is_ascii_digit)
        {
            continue;
        }
        let path = entry.path();
        if !fs::metadata(&path).is_ok_and(|m| m.uid() == uid) {
            continue;
        }
        // Zombies no longer render. Inaccessible or exiting processes are skipped.
        if fs::read_to_string(path.join("stat")).is_ok_and(|s| {
            s.rsplit_once(") ")
                .is_some_and(|(_, tail)| tail.starts_with('Z'))
        }) {
            continue;
        }
        let Ok(file) = fs::File::open(path.join("environ")) else {
            continue;
        };
        let mut bytes = Vec::new();
        if file.take(128 * 1024).read_to_end(&mut bytes).is_ok() && steam_game(&bytes) {
            return Ok(true);
        }
    }
    Ok(false)
}

type Observation = Option<(Instant, std::result::Result<bool, String>)>;
pub struct Watcher {
    latest: Arc<Mutex<Observation>>,
    enabled: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
}
impl Watcher {
    pub fn start() -> Result<Self> {
        let watcher = Self {
            latest: Arc::new(Mutex::new(None)),
            enabled: Arc::new(AtomicBool::new(false)),
            alive: Arc::new(AtomicBool::new(true)),
        };
        let latest = watcher.latest.clone();
        let enabled = watcher.enabled.clone();
        let alive = watcher.alive.clone();
        let uid = nix::unistd::geteuid().as_raw();
        thread::Builder::new()
            .name("game-detection".into())
            .spawn(move || {
                while alive.load(Ordering::Relaxed) {
                    let observation = enabled.load(Ordering::Relaxed).then(|| {
                        (
                            Instant::now(),
                            scan(Path::new("/proc"), uid).map_err(|e| format!("{e:#}")),
                        )
                    });
                    if let Ok(mut value) = latest.lock() {
                        *value = observation;
                    }
                    thread::sleep(Duration::from_millis(750));
                }
            })?;
        Ok(watcher)
    }
    pub fn enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }
    pub fn snapshot(&self) -> std::result::Result<Option<bool>, String> {
        let value = self
            .latest
            .lock()
            .map_err(|_| "Game detection unavailable")?;
        match value.as_ref() {
            Some((at, value)) if at.elapsed() < Duration::from_secs(4) => value.clone().map(Some),
            Some(_) => Err("Game detection expired; keeping wallpapers unloaded".into()),
            None => Ok(None),
        }
    }
}
impl Drop for Watcher {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn steam_games_include_proton_and_shortcuts_but_not_workshop_or_invalid_ids() {
        for value in [
            b"SteamAppId=1422450\0".as_slice(),
            b"SteamGameId=123456789123456\0",
        ] {
            assert!(steam_game(value));
        }
        for value in [
            b"SteamAppId=431960\0".as_slice(),
            b"SteamGameId=0\0",
            b"NotSteamAppId=1422450\0",
            b"SteamAppId=oops\0",
            b"SteamAppId=228980\0",
        ] {
            assert!(!steam_game(value));
        }
    }
    #[test]
    fn process_exit_zombies_and_missing_environment_are_handled() {
        let dir = tempfile::tempdir().expect("fixture");
        let pid = dir.path().join("123");
        fs::create_dir(&pid).expect("pid");
        let uid = fs::metadata(&pid).expect("metadata").uid();
        assert!(!scan(dir.path(), uid).expect("scan"));
        fs::write(pid.join("environ"), b"SteamAppId=1422450\0").expect("env");
        assert!(scan(dir.path(), uid).expect("scan"));
        fs::write(pid.join("stat"), "123 (game ) name) Z 1 2").expect("stat");
        assert!(!scan(dir.path(), uid).expect("scan"));
        fs::remove_dir_all(pid).expect("exit");
        assert!(!scan(dir.path(), uid).expect("scan"));
    }
}
