//! Session-local supervisor. Only child process groups started here are signalled.
use crate::{
    config::{Assignment, Paths},
    library, renderer,
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
    pub log: PathBuf,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    pub message: String,
    pub states: Vec<RunState>,
    #[serde(default)]
    pub failures: Vec<String>,
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
        if matches!(request, Request::Restore { .. }) {
            60
        } else {
            15
        },
    )))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    serde_json::to_writer(&mut stream, request)?;
    stream.write_all(b"\n")?;
    read_message(&stream)
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
        ensure!(
            self.child.try_wait()?.is_none(),
            "Wallpaper renderer has already exited"
        );
        killpg(
            Pid::from_raw(self.child.id() as i32),
            if paused {
                Signal::SIGSTOP
            } else {
                Signal::SIGCONT
            },
        )?;
        self.state.paused = paused;
        Ok(())
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Supervisor {
    paths: Paths,
    active: BTreeMap<String, Running>,
    failures: Vec<String>,
    relocation_at: Instant,
}

impl Supervisor {
    fn poll(&mut self) {
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
        if self.failures.len() > 20 {
            self.failures.drain(..self.failures.len() - 20);
        }
    }
    fn states(&self) -> Vec<RunState> {
        self.active.values().map(|v| v.state.clone()).collect()
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
        let plan = renderer::plan(&engine, &config, wallpaper, output, preview, properties)?;
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
            paused: false,
            log: log_path.clone(),
        };
        let mut run = Running {
            child,
            state,
            staged: plan.wallpaper.staged,
            properties: properties.clone(),
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
        Ok(format!(
            "Renderer started for {}. Rendering correctness still needs visual verification.",
            plan.output
        ))
    }
    fn dispatch(&mut self, request: Request) -> Result<String> {
        match request {
            Request::Apply {
                target,
                output,
                preview,
                properties,
            } => self.apply(&target, &output, preview, &properties),
            Request::Status => Ok(if self.active.is_empty() {
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
                    if self.active.contains_key(&output) {
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
                } else {
                    self.active.clear();
                }
                Ok("Stopped Wallweave's wallpaper processes".into())
            }
            Request::Shutdown => {
                self.active.clear();
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
                    .filter(|key| output.as_ref().is_none_or(|o| o == *key))
                    .cloned()
                    .collect();
                if selected.is_empty() {
                    Err(anyhow::anyhow!("No matching wallpaper is running"))
                } else {
                    let result: Result<()> = selected.into_iter().try_for_each(|key| {
                        self.active
                            .get_mut(&key)
                            .context("Wallpaper disappeared")?
                            .pause(paused)
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
    let mut supervisor = Supervisor {
        paths: paths.clone(),
        active: BTreeMap::new(),
        failures: vec![],
        relocation_at: Instant::now(),
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
