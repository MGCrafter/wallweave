//! Session-local supervisor. Only child process groups started here are signalled.
use crate::{
    config::{Assignment, AutoPause, GamingMode, Paths},
    gaming, library, renderer, visibility,
};
use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Request {
    Apply {
        target: String,
        output: String,
        preview: bool,
        #[serde(default)]
        properties: BTreeMap<String, String>,
    },
    Status,
    /// Apply saved game-unload and auto-pause rules to desktop wallpapers.
    ReloadPlayback,
    /// Start saved wallpapers only on connected outputs without a live renderer.
    Restore {
        outputs: Vec<String>,
    },
    Pause {
        output: Option<String>,
    },
    Resume {
        output: Option<String>,
    },
    Stop {
        output: Option<String>,
    },
    Shutdown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunState {
    pub output: String,
    pub title: String,
    pub wallpaper: PathBuf,
    pub pid: u32,
    pub paused: bool,
    /// Effective renderer rule, not a report of its instantaneous pause state.
    #[serde(default)]
    pub auto_pause: Option<AutoPause>,
    /// Separate from manual pause: None means renderer-managed detection.
    #[serde(default)]
    pub auto_paused: Option<bool>,
    /// A cooperative renderer control is available; no process suspension.
    #[serde(default)]
    pub pause_supported: bool,
    pub log: PathBuf,
    /// No renderer process exists; pid is zero until restoration succeeds.
    #[serde(default)]
    pub unloaded: bool,
}

impl RunState {
    pub fn playback_label(&self) -> &'static str {
        if self.unloaded && self.paused {
            "Unloaded · manually paused"
        } else if self.unloaded {
            "Unloaded · waiting to restore"
        } else if self.paused {
            "Paused"
        } else if self.auto_paused == Some(true) {
            "Paused automatically"
        } else if self.auto_paused == Some(false) {
            "Playing"
        } else if matches!(
            self.auto_pause,
            Some(AutoPause::Focused | AutoPause::Fullscreen)
        ) {
            "Auto-pause enabled"
        } else {
            "Playing"
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    pub message: String,
    pub states: Vec<RunState>,
    #[serde(default)]
    pub failures: Vec<String>,
    #[serde(default)]
    pub gaming_supported: bool,
    #[serde(default)]
    pub game_active: bool,
}

fn read_message<T: for<'de> Deserialize<'de>>(stream: &UnixStream) -> Result<T> {
    let mut line = String::new();
    BufReader::new(stream.take(1024 * 1024 + 1)).read_line(&mut line)?;
    ensure!(line.len() <= 1024 * 1024, "Control message exceeds 1 MiB");
    serde_json::from_str(&line).context("Invalid control message")
}

pub fn client(paths: &Paths, request: &Request) -> Result<Response> {
    let mut stream = UnixStream::connect(paths.socket()).context("Wallweave is not running")?;
    stream.set_read_timeout(Some(Duration::from_secs(
        if matches!(request, Request::Restore { .. } | Request::ReloadPlayback) {
            60
        } else {
            15
        },
    )))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    serde_json::to_writer(&mut stream, request)?;
    stream.write_all(b"\n")?;
    let mut response: Response = read_message(&stream)?;
    if matches!(request, Request::ReloadPlayback)
        && response
            .message
            .contains("unknown variant `reload_playback`")
    {
        response.message = "Auto-pause preference saved. The running supervisor is from an older version: run wallweave shutdown, then reopen Wallweave to restore your wallpapers with the new rules.".into();
    }
    if matches!(request, Request::ReloadPlayback)
        && !response.gaming_supported
        && GamingMode::load(paths).unwrap_or_default() == GamingMode::Unload
    {
        response.ok = false;
        response.message = "Spielmodus gespeichert. Bitte zuerst „wallweave shutdown“ ausführen und dann die aktualisierte App öffnen, damit die Hintergrundsteuerung ihn übernimmt.".into();
    }
    Ok(response)
}

pub fn ensure_running(paths: &Paths) -> Result<()> {
    if client(paths, &Request::Status).is_ok() {
        return Ok(());
    }
    fs::create_dir_all(&paths.state)?;
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.state.join("supervisor.log"))?;
    Command::new(std::env::current_exe()?)
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .process_group(0)
        .spawn()?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if client(paths, &Request::Status).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(40));
    }
    bail!(
        "Supervisor did not start; check {}",
        paths.state.join("supervisor.log").display()
    )
}

/// Restore once when the GUI opens, including when another GUI is also opening.
/// Keep the current protocol usable with an older, already-running supervisor.
pub fn restore_saved(paths: &Paths, outputs: &[String]) -> Result<Response> {
    let assignments: Vec<_> = paths
        .load()?
        .assignments
        .into_iter()
        .filter(|(output, _)| output != "preview" && outputs.contains(output))
        .collect();
    if assignments.is_empty() {
        let mut response = client(paths, &Request::Status).unwrap_or_default();
        response.ok = true;
        response.message = "No saved wallpapers for the connected displays.".into();
        return Ok(response);
    }
    ensure_running(paths)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(paths.runtime.join("restore.lock"))?;
    lock.lock_exclusive()?;
    let mut response = client(paths, &Request::Status)?;
    ensure!(
        response.ok,
        "Cannot inspect running wallpapers: {}",
        response.message
    );
    if assignments
        .iter()
        .all(|(output, _)| response.states.iter().any(|state| &state.output == output))
    {
        response.message = "Saved wallpapers are already running.".into();
        return Ok(response);
    }
    let restored = client(
        paths,
        &Request::Restore {
            outputs: outputs.to_vec(),
        },
    )?;
    if !restored.message.contains("unknown variant `restore`") {
        return Ok(restored);
    }
    // An older supervisor can keep playing throughout an app update. Recheck
    // each output before using its existing Apply command, without restarting it.
    let mut errors = vec![];
    let mut count = 0;
    for (output, assignment) in assignments {
        response = client(paths, &Request::Status)?;
        ensure!(
            response.ok,
            "Cannot inspect running wallpapers: {}",
            response.message
        );
        if response.states.iter().any(|state| state.output == output) {
            continue;
        }
        response = client(
            paths,
            &Request::Apply {
                target: assignment.wallpaper.to_string_lossy().into_owned(),
                output: output.clone(),
                preview: false,
                properties: assignment.properties,
            },
        )?;
        if response.ok {
            count += 1;
        } else {
            errors.push(format!("{output}: {}", response.message));
        }
    }
    response.ok = errors.is_empty();
    response.message = format!(
        "Restored {count} saved wallpaper(s).{}",
        if errors.is_empty() {
            String::new()
        } else {
            format!("\n{}", errors.join("\n"))
        }
    );
    Ok(response)
}

struct Running {
    child: Child,
    state: RunState,
    staged: bool,
    properties: BTreeMap<String, String>,
    requested_auto_pause: AutoPause,
    pause_control: Option<PauseControl>,
    pause_refresh_at: Instant,
}

/// One private file per renderer. Atomic replacement avoids partial commands;
/// removing the directory on shutdown also releases a surviving renderer.
struct PauseControl(tempfile::TempDir);

impl PauseControl {
    fn new(paths: &Paths) -> Result<Self> {
        let control = Self(
            tempfile::Builder::new()
                .prefix("pause-")
                .tempdir_in(&paths.runtime)?,
        );
        fs::set_permissions(control.0.path(), fs::Permissions::from_mode(0o700))?;
        control.set(false)?;
        Ok(control)
    }

    fn path(&self) -> PathBuf {
        self.0.path().join("state")
    }

    fn set(&self, paused: bool) -> Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(self.0.path())?;
        file.write_all(if paused { b"pause\n" } else { b"play\n" })?;
        file.persist(self.path()).map_err(|e| e.error)?;
        Ok(())
    }
}

impl Running {
    fn stop(&mut self) {
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            return;
        }
        let group = Pid::from_raw(self.child.id() as i32);
        let _ = killpg(group, Signal::SIGCONT);
        let _ = killpg(group, Signal::SIGTERM);
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(1200) {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = killpg(group, Signal::SIGKILL);
        let _ = self.child.wait();
    }
    fn pause(&mut self, paused: bool) -> Result<()> {
        self.set_pause(paused, self.state.auto_paused)
    }
    fn automatic_pause(&mut self, paused: bool) -> Result<()> {
        self.set_pause(self.state.paused, Some(paused))
    }
    fn set_pause(&mut self, manual: bool, automatic: Option<bool>) -> Result<()> {
        ensure!(
            self.child.try_wait()?.is_none(),
            "Wallpaper renderer has already exited"
        );
        let was_paused = self.state.paused || self.state.auto_paused == Some(true);
        let paused = manual || automatic == Some(true);
        if was_paused != paused {
            self.pause_control.as_ref().context(
                "This renderer cannot pause safely. Install a renderer with --pause-control-file support; process suspension is disabled to protect system audio."
            )?.set(paused)?;
            self.pause_refresh_at = Instant::now();
        }
        self.state.paused = manual;
        self.state.auto_paused = automatic;
        Ok(())
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone)]
struct Unloaded {
    state: RunState,
    properties: BTreeMap<String, String>,
}

struct Supervisor {
    paths: Paths,
    active: BTreeMap<String, Running>,
    failures: Vec<String>,
    relocation_at: Instant,
    visibility: Option<visibility::Watcher>,
    visibility_error: Option<String>,
    gaming: Option<gaming::Watcher>,
    gaming_mode: GamingMode,
    game_active: bool,
    game_quiet_since: Option<Instant>,
    gaming_error: Option<String>,
    unloaded: BTreeMap<String, Unloaded>,
    restore_at: Option<Instant>,
}

impl Supervisor {
    fn poll(&mut self) {
        self.poll_gaming();
        // The renderer expires a pause after three seconds without renewal.
        // A crashed supervisor therefore cannot strand playback in pause.
        for run in self.active.values_mut() {
            if (run.state.paused || run.state.auto_paused == Some(true))
                && run.pause_refresh_at.elapsed() >= Duration::from_millis(500)
            {
                if let Some(control) = &run.pause_control
                    && let Err(error) = control.set(true)
                {
                    self.failures
                        .push(format!("Renewing pause on {}: {error:#}", run.state.output));
                }
                run.pause_refresh_at = Instant::now();
            }
        }
        if self.relocation_at.elapsed() >= Duration::from_secs(2) {
            self.relocation_at = Instant::now();
            let moved: Vec<_> = self
                .active
                .values()
                .filter_map(|run| {
                    if !run.staged || run.state.wallpaper.is_dir() {
                        return None;
                    }
                    let installed = library::installed_folder(&run.state.wallpaper)?;
                    installed.join("project.json").is_file().then(|| {
                        (
                            installed,
                            run.state.output.clone(),
                            run.properties.clone(),
                            run.state.paused,
                        )
                    })
                })
                .collect();
            for (folder, output, properties, paused) in moved {
                match self.apply(
                    &folder.to_string_lossy(),
                    &output,
                    output == "preview",
                    &properties,
                ) {
                    Ok(_) => {
                        if paused
                            && let Some(run) = self.active.get_mut(&output)
                            && let Err(error) = run.pause(true)
                        {
                            self.failures.push(format!(
                                "Restoring pause after Steam installation: {error:#}"
                            ));
                        }
                    }
                    Err(error) => self.failures.push(format!(
                        "Following Steam's installed wallpaper on {output}: {error:#}"
                    )),
                }
            }
        }
        let finished: Vec<_> = self
            .active
            .iter_mut()
            .filter_map(|(key, run)| match run.child.try_wait() {
                Ok(Some(status)) => Some((
                    key.clone(),
                    format!(
                        "{} exited ({status}); log: {}",
                        run.state.title,
                        run.state.log.display()
                    ),
                )),
                Err(e) => Some((
                    key.clone(),
                    format!("Cannot inspect {}: {e}", run.state.title),
                )),
                Ok(None) => None,
            })
            .collect();
        for (key, message) in finished {
            self.active.remove(&key);
            self.failures.push(message);
        }
        if let Some(watcher) = &self.visibility {
            let enabled = self.active.values().any(|run| {
                run.state.auto_paused.is_some() && run.state.auto_pause != Some(AutoPause::Off)
            });
            watcher.enabled(
                enabled || self.gaming_mode == GamingMode::Unload || !self.unloaded.is_empty(),
            );
            let observation = if enabled {
                watcher.snapshot()
            } else {
                Ok(visibility::Snapshot::default())
            };
            let error = observation.as_ref().err().cloned();
            if error != self.visibility_error {
                if let Some(error) = &error {
                    self.failures.push(format!(
                        "Automatic pause detection unavailable; automatic pauses released: {error}"
                    ));
                }
                self.visibility_error = error;
            }
            let snapshot = observation.unwrap_or_default();
            for run in self
                .active
                .values_mut()
                .filter(|run| run.state.auto_paused.is_some())
            {
                let paused = snapshot.pauses(
                    &run.state.output,
                    run.state.auto_pause.unwrap_or(AutoPause::Off),
                );
                if run.state.auto_paused != Some(paused)
                    && let Err(error) = run.automatic_pause(paused)
                {
                    self.failures.push(format!(
                        "Automatic pause on {}: {error:#}",
                        run.state.output
                    ));
                }
            }
        }
        if self.failures.len() > 20 {
            self.failures.drain(..self.failures.len() - 20);
        }
    }
    fn poll_gaming(&mut self) {
        let enabled = self.gaming_mode == GamingMode::Unload;
        if let Some(watcher) = &self.gaming {
            watcher.enabled(enabled);
        }
        if let Some(watcher) = &self.visibility
            && (enabled || !self.unloaded.is_empty())
        {
            watcher.enabled(true);
        }
        let detected = if !enabled {
            Ok(Some(false))
        } else {
            let steam = self
                .gaming
                .as_ref()
                .map(|w| w.snapshot())
                .unwrap_or(Ok(None));
            let window = self
                .visibility
                .as_ref()
                .and_then(|w| w.snapshot().ok())
                .is_some_and(|s| s.game_running());
            if window { Ok(Some(true)) } else { steam }
        };
        let error = detected.as_ref().err().cloned();
        if error != self.gaming_error {
            if let Some(error) = &error {
                self.failures.push(format!("Game detection: {error}"));
            }
            self.gaming_error = error;
        }
        self.update_gaming(detected.ok().flatten());
    }

    fn update_gaming(&mut self, detected: Option<bool>) {
        if self.gaming_mode == GamingMode::Off {
            self.game_active = false;
            self.game_quiet_since = None;
        } else if detected == Some(true) {
            self.game_active = true;
            self.game_quiet_since = None;
        } else if detected == Some(false) {
            let quiet = self.game_quiet_since.get_or_insert_with(Instant::now);
            if quiet.elapsed() >= Duration::from_secs(3) {
                self.game_active = false;
            }
        } else {
            // Unknown/stale detection must not restart renderers during a game.
            self.game_active = true;
            self.game_quiet_since = None;
        }
        if self.game_active {
            self.restore_at = None;
            let outputs: Vec<_> = self
                .active
                .keys()
                .filter(|key| key.as_str() != "preview")
                .cloned()
                .collect();
            for output in outputs {
                if let Some(run) = self.active.remove(&output) {
                    let mut state = run.state.clone();
                    state.pid = 0;
                    state.unloaded = true;
                    self.unloaded.insert(
                        output,
                        Unloaded {
                            state,
                            properties: run.properties.clone(),
                        },
                    );
                    drop(run); // Terminate and reap our own renderer group, releasing GPU allocations.
                }
            }
        } else if !self.unloaded.is_empty()
            && self
                .restore_at
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(5))
        {
            self.restore_at = Some(Instant::now());
            let pending = self.unloaded.clone();
            for (output, saved) in pending {
                if let Some(watcher) = &self.visibility {
                    // A disconnected output stays queued for reconnect, without a restart loop.
                    if !watcher.snapshot().is_ok_and(|s| s.connected(&output)) {
                        continue;
                    }
                }
                match self.apply(
                    &saved.state.wallpaper.to_string_lossy(),
                    &output,
                    false,
                    &saved.properties,
                ) {
                    Ok(_) => {}
                    Err(e) => {
                        let message = format!("Restore {output} after game: {e:#}");
                        if !self.failures.contains(&message) {
                            self.failures.push(message);
                        }
                    }
                }
            }
        }
    }

    fn states(&self) -> Vec<RunState> {
        self.active
            .values()
            .map(|v| v.state.clone())
            .chain(self.unloaded.values().map(|v| v.state.clone()))
            .collect()
    }
    fn apply(
        &mut self,
        target: &str,
        output: &str,
        preview: bool,
        properties: &BTreeMap<String, String>,
    ) -> Result<String> {
        let mut config = self.paths.load()?;
        let wallpaper = library::resolve(target, &config)?;
        let engine = renderer::Renderer::detect(&config)?;
        let mut plan = renderer::plan(&engine, &config, wallpaper, output, preview, properties)?;
        let requested_auto_pause = AutoPause::load(&self.paths)?;
        if !preview && self.game_active {
            let state = RunState {
                output: output.into(),
                title: plan.wallpaper.title.clone(),
                wallpaper: plan.wallpaper.folder.clone(),
                pid: 0,
                paused: false,
                auto_pause: Some(requested_auto_pause),
                auto_paused: None,
                pause_supported: engine.supports_pause_control(),
                log: self.paths.state.join("logs").join(format!("{output}.log")),
                unloaded: true,
            };
            config.assignments.insert(
                output.into(),
                Assignment {
                    wallpaper: state.wallpaper.clone(),
                    properties: properties.clone(),
                },
            );
            self.paths.save(&config)?;
            self.active.remove(output);
            self.unloaded.insert(
                output.into(),
                Unloaded {
                    state,
                    properties: properties.clone(),
                },
            );
            return Ok("Wallpaper saved; unloaded while a game is running. It will return after the game exits.".into());
        }
        let workspace_aware =
            self.visibility.is_some() && !preview && engine.supports("--no-fullscreen-pause");
        let auto_pause = plan.auto_pause(&engine, requested_auto_pause, workspace_aware);
        // Set the initial control before spawning: restoring a manually paused
        // wallpaper must not briefly play its audio or animation.
        let manually_paused = !preview
            && self
                .unloaded
                .get(output)
                .is_some_and(|saved| saved.state.paused);
        ensure!(
            !manually_paused || engine.supports_pause_control(),
            "Cannot restore manual pause: renderer lacks safe pause control"
        );
        let pause_control = if engine.supports_pause_control() {
            let control = PauseControl::new(&self.paths)?;
            control.set(manually_paused)?;
            plan.arguments.extend([
                "--pause-control-file".into(),
                control.path().into_os_string(),
            ]);
            Some(control)
        } else {
            None
        };
        let key = plan.output.clone();
        fs::create_dir_all(self.paths.state.join("logs"))?;
        let safe: String = key
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .take(120)
            .collect();
        let log_path = self.paths.state.join("logs").join(format!("{safe}.log"));
        if log_path.exists() {
            let _ = fs::rename(&log_path, log_path.with_extension("previous.log"));
        }
        let log = fs::File::create(&log_path)?;
        let child = plan
            .command()
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .spawn()
            .context("Launch wallpaper renderer")?;
        let state = RunState {
            output: key.clone(),
            title: plan.wallpaper.title.clone(),
            wallpaper: plan.wallpaper.folder.clone(),
            pid: child.id(),
            paused: manually_paused,
            auto_pause,
            auto_paused: workspace_aware.then_some(false),
            pause_supported: pause_control.is_some(),
            log: log_path.clone(),
            unloaded: false,
        };
        let mut run = Running {
            child,
            state,
            staged: plan.wallpaper.staged,
            properties: properties.clone(),
            requested_auto_pause,
            pause_control,
            pause_refresh_at: Instant::now(),
        };
        // Keep the current wallpaper if the replacement fails during startup.
        thread::sleep(Duration::from_millis(350));
        if let Some(status) = run.child.try_wait()? {
            let text = tail(&log_path, 2400);
            bail!(
                "Renderer exited during startup ({status}).\n{text}\nLog: {}",
                log_path.display()
            );
        }
        if workspace_aware && let Some(watcher) = &self.visibility {
            let paused = watcher
                .snapshot()
                .unwrap_or_default()
                .pauses(&key, auto_pause.unwrap_or(AutoPause::Off));
            run.automatic_pause(paused)?;
        }
        self.unloaded.remove(&key);
        self.active.insert(key, run);
        if !preview {
            config.assignments.insert(
                output.into(),
                Assignment {
                    wallpaper: plan.wallpaper.folder,
                    properties: properties.clone(),
                },
            );
            if let Err(error) = self.paths.save(&config) {
                return Ok(format!(
                    "Renderer started; could not save assignment: {error:#}"
                ));
            }
        }
        let fallback = if workspace_aware
            && auto_pause == Some(AutoPause::Off)
            && requested_auto_pause != AutoPause::Off
        {
            " Automatic pause is unavailable: this renderer lacks safe pause control. Wallpapers keep playing to protect system audio."
        } else if auto_pause.is_some_and(|mode| mode != requested_auto_pause) {
            " The renderer does not support the selected auto-pause rule; using its default fullscreen pause."
        } else {
            ""
        };
        Ok(format!(
            "Renderer started for {}. Rendering correctness still needs visual verification.{fallback}",
            plan.output
        ))
    }
    fn dispatch(&mut self, request: Request) -> Result<String> {
        match request {
            Request::ReloadPlayback => {
                self.gaming_mode = GamingMode::load(&self.paths)?;
                self.poll_gaming();
                let mode = AutoPause::load(&self.paths)?;
                let targets: Vec<_> = self
                    .active
                    .values()
                    .filter(|run| run.state.output != "preview" && run.requested_auto_pause != mode)
                    .map(|run| (run.state.clone(), run.properties.clone()))
                    .collect();
                let mut updated = 0;
                let mut errors = vec![];
                for (state, properties) in targets {
                    match self.apply(
                        &state.wallpaper.to_string_lossy(),
                        &state.output,
                        false,
                        &properties,
                    ) {
                        Ok(message) => {
                            updated += 1;
                            if message.contains("could not save assignment") {
                                errors.push(message);
                            }
                            if state.paused
                                && let Some(run) = self.active.get_mut(&state.output)
                                && let Err(error) = run.pause(true)
                            {
                                errors.push(format!(
                                    "{}: restore manual pause: {error:#}",
                                    state.output
                                ));
                            }
                        }
                        Err(error) => errors.push(format!("{}: {error:#}", state.output)),
                    }
                }
                let message = format!("Playback rules updated for {updated} wallpaper(s).");
                ensure!(errors.is_empty(), "{message}\n{}", errors.join("\n"));
                Ok(message)
            }
            Request::Apply {
                target,
                output,
                preview,
                properties,
            } => self.apply(&target, &output, preview, &properties),
            Request::Status => Ok(if self.game_active && !self.unloaded.is_empty() {
                "Game running; wallpapers unloaded"
            } else if self.active.is_empty() {
                "No wallpapers running"
            } else {
                "Running"
            }
            .into()),
            Request::Restore { outputs } => {
                let assignments = self.paths.load()?.assignments;
                let mut restored = 0;
                let mut running = 0;
                let mut errors = vec![];
                for (output, assignment) in assignments {
                    if output == "preview" || !outputs.contains(&output) {
                        continue;
                    }
                    self.poll();
                    if self.active.contains_key(&output) || self.unloaded.contains_key(&output) {
                        running += 1;
                        continue;
                    }
                    match self.apply(
                        &assignment.wallpaper.to_string_lossy(),
                        &output,
                        false,
                        &assignment.properties,
                    ) {
                        Ok(_) => restored += 1,
                        Err(error) => errors.push(format!("{output}: {error:#}")),
                    }
                }
                let message =
                    format!("Restored {restored} saved wallpaper(s); {running} already running.");
                ensure!(errors.is_empty(), "{message}\n{}", errors.join("\n"));
                Ok(message)
            }
            Request::Pause { output } | Request::Resume { output } => {
                // The variant is dispatched separately below, before this method.
                let _ = output;
                unreachable!("pause/resume handled by handle")
            }
            Request::Stop { output } => {
                if let Some(output) = output {
                    self.active.remove(&output);
                    self.unloaded.remove(&output);
                } else {
                    self.active.clear();
                    self.unloaded.clear();
                }
                Ok("Stopped Wallweave's wallpaper processes".into())
            }
            Request::Shutdown => {
                self.active.clear();
                self.unloaded.clear();
                Ok("Wallweave supervisor stopped".into())
            }
        }
    }
    fn handle(&mut self, request: Request) -> Response {
        self.poll();
        let result = match &request {
            Request::Pause { output } | Request::Resume { output } => {
                let paused = matches!(request, Request::Pause { .. });
                let selected: Vec<_> = self
                    .active
                    .keys()
                    .chain(self.unloaded.keys())
                    .filter(|key| output.as_ref().is_none_or(|o| o == *key))
                    .cloned()
                    .collect();
                if selected.is_empty() {
                    Err(anyhow::anyhow!("No matching wallpaper is running"))
                } else {
                    let result: Result<()> = selected.into_iter().try_for_each(|key| {
                        if let Some(run) = self.unloaded.get_mut(&key) {
                            ensure!(
                                !paused || run.state.pause_supported,
                                "This renderer cannot pause safely"
                            );
                            run.state.paused = paused;
                            Ok(())
                        } else {
                            self.active
                                .get_mut(&key)
                                .context("Wallpaper disappeared")?
                                .pause(paused)
                        }
                    });
                    result.map(|_| {
                        if paused {
                            "Paused".into()
                        } else {
                            "Resumed".into()
                        }
                    })
                }
            }
            _ => self.dispatch(request),
        };
        Response {
            ok: result.is_ok(),
            message: match result {
                Ok(text) => text,
                Err(e) => format!("{e:#}"),
            },
            states: self.states(),
            failures: self.failures.clone(),
            gaming_supported: true,
            game_active: self.game_active,
        }
    }
}

pub fn tail(path: &std::path::Path, max: usize) -> String {
    use std::io::{Seek, SeekFrom};
    let Ok(mut file) = fs::File::open(path) else {
        return String::new();
    };
    let len = file.metadata().map(|v| v.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(len.saturating_sub(max as u64)));
    let mut bytes = vec![];
    let _ = file.take(max as u64).read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

pub fn run(paths: Paths) -> Result<()> {
    fs::create_dir_all(&paths.runtime)?;
    let metadata = fs::symlink_metadata(&paths.runtime)?;
    ensure!(
        !metadata.file_type().is_symlink() && metadata.uid() == nix::unistd::geteuid().as_raw(),
        "Runtime directory is not owned by this user"
    );
    fs::set_permissions(&paths.runtime, fs::Permissions::from_mode(0o700))?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(paths.runtime.join("supervisor.lock"))?;
    lock.try_lock_exclusive()
        .context("Another Wallweave supervisor is already running")?;
    if paths.socket().exists() {
        fs::remove_file(paths.socket())?;
    }
    let listener = UnixListener::bind(paths.socket())?;
    fs::set_permissions(paths.socket(), fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let alive = Arc::new(AtomicBool::new(true));
    let flag = alive.clone();
    ctrlc::set_handler(move || {
        flag.store(false, Ordering::Relaxed);
    })?;
    let gaming_mode = GamingMode::load(&paths)?;
    let mut supervisor = Supervisor {
        paths: paths.clone(),
        active: BTreeMap::new(),
        failures: vec![],
        relocation_at: Instant::now(),
        visibility: visibility::Watcher::start(),
        visibility_error: None,
        gaming: Some(gaming::Watcher::start()?),
        gaming_mode,
        game_active: gaming_mode == GamingMode::Unload,
        game_quiet_since: None,
        gaming_error: None,
        unloaded: BTreeMap::new(),
        restore_at: None,
    };
    while alive.load(Ordering::Relaxed) {
        supervisor.poll();
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                let request = read_message::<Request>(&stream);
                let shutdown = matches!(request, Ok(Request::Shutdown));
                let response = match request {
                    Ok(request) => supervisor.handle(request),
                    Err(error) => Response {
                        message: format!("{error:#}"),
                        ..Response::default()
                    },
                };
                let _ = serde_json::to_writer(&mut stream, &response);
                let _ = stream.write_all(b"\n");
                if shutdown {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(40))
            }
            Err(e) => return Err(e.into()),
        }
    }
    drop(supervisor);
    let _ = fs::remove_file(paths.socket());
    Ok(())
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
