use crate::{
    config::{AutoPause, Config},
    desktop,
    library::{self, Wallpaper},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Renderer {
    pub executable: PathBuf,
    pub hot_swap: bool,
    pub web_frame_transfer: bool,
    pub supported_flags: Vec<String>,
}

impl Renderer {
    pub fn detect(config: &Config) -> Result<Self> {
        let executable = config.renderer.clone().or_else(bundled_renderer).or_else(|| desktop::find_program("linux-wallpaperengine"))
            .context("No renderer found. Run scripts/install-renderer.py, or set the renderer executable in Settings.")?;
        ensure!(
            executable.is_file(),
            "Renderer unavailable: {}",
            executable.display()
        );
        let mut child = Command::new(&executable)
            .arg("--help")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("Start renderer help: {}", executable.display()))?;
        // Read concurrently so large help output cannot fill the pipe and deadlock.
        let stdout = child.stdout.take().context("Renderer help pipe missing")?;
        let reader = thread::spawn(move || {
            use std::io::Read;
            let mut text = String::new();
            let _ = stdout.take(128 * 1024).read_to_string(&mut text);
            text
        });
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if start.elapsed() > Duration::from_secs(4) {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("Renderer did not answer --help within 4 seconds");
            }
            thread::sleep(Duration::from_millis(20));
        };
        ensure!(status.success(), "Renderer --help failed: {status}");
        let help = reader
            .join()
            .map_err(|_| anyhow::anyhow!("Renderer help reader failed"))?;
        let flags = [
            "--window",
            "--screen-root",
            "--bg",
            "--fps",
            "--assets-dir",
            "--set-property",
            "--scaling",
            "--silent",
            "--volume",
            "--control-file",
            "--pause-control-file",
            "--web-frames",
            "--layer",
            "--no-fullscreen-pause",
            "--pause-on-focus",
            "--pause-on-maximized",
        ];
        let supported_flags: Vec<_> = flags
            .into_iter()
            .filter(|flag| help.contains(flag))
            .map(str::to_owned)
            .collect();
        ensure!(
            supported_flags.iter().any(|s| s == "--assets-dir")
                && supported_flags.iter().any(|s| s == "--screen-root"),
            "Executable is not a supported linux-wallpaperengine renderer"
        );
        Ok(Self {
            executable,
            hot_swap: help.contains("--control-file"),
            web_frame_transfer: help.contains("--web-frames"),
            supported_flags,
        })
    }
    pub fn supports(&self, flag: &str) -> bool {
        self.supported_flags.iter().any(|s| s == flag)
    }

    pub fn supports_auto_pause(&self, mode: AutoPause) -> bool {
        match mode {
            AutoPause::Off => self.supports("--no-fullscreen-pause"),
            // Fullscreen detection is part of the basic renderer. The extended
            // maximized-window rule is optional on wlroots desktops.
            AutoPause::Fullscreen => true,
            AutoPause::Focused => self.supports("--pause-on-focus"),
        }
    }

    pub fn supports_pause_control(&self) -> bool {
        self.supports("--pause-control-file")
    }
}

fn bundled_renderer() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    // A distributable binary next to runtime/, or cargo's target/{debug,release}.
    for base in executable.ancestors().skip(1).take(4) {
        let path = base.join("runtime/renderer-path.txt");
        if let Ok(text) = std::fs::read_to_string(path) {
            let candidate = PathBuf::from(text.trim());
            let candidate = if candidate.is_absolute() {
                candidate
            } else {
                base.join("runtime").join(candidate)
            };
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LaunchPlan {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub wallpaper: Wallpaper,
    pub output: String,
}

impl LaunchPlan {
    /// Preview windows stay independent of desktop pause rules. Unsupported
    /// focus rules fall back to the renderer's existing fullscreen behavior.
    pub fn auto_pause(
        &mut self,
        renderer: &Renderer,
        mode: AutoPause,
        workspace_aware: bool,
    ) -> Option<AutoPause> {
        if self.output == "preview" {
            if renderer.supports("--no-fullscreen-pause") {
                self.arguments.push("--no-fullscreen-pause".into());
            }
            return None;
        }
        if workspace_aware && renderer.supports("--no-fullscreen-pause") {
            // Our supervisor checks Hyprland workspace visibility; stale
            // foreign-toplevel flags must not independently keep it paused.
            self.arguments.push("--no-fullscreen-pause".into());
            // Never substitute process suspension for cooperative renderer
            // pause: stopping audio threads can stall the system audio graph.
            return Some(if renderer.supports_pause_control() {
                mode
            } else {
                AutoPause::Off
            });
        }
        let effective = if renderer.supports_auto_pause(mode) {
            mode
        } else {
            AutoPause::Fullscreen
        };
        match effective {
            AutoPause::Off => self.arguments.push("--no-fullscreen-pause".into()),
            AutoPause::Fullscreen | AutoPause::Focused => {
                if effective == AutoPause::Focused {
                    self.arguments.push("--pause-on-focus".into());
                }
                if renderer.supports("--pause-on-maximized") {
                    self.arguments.push("--pause-on-maximized".into());
                }
            }
        }
        Some(effective)
    }
}

pub fn plan(
    renderer: &Renderer,
    config: &Config,
    wallpaper: Wallpaper,
    output: &str,
    preview: bool,
    properties: &BTreeMap<String, String>,
) -> Result<LaunchPlan> {
    config.validate()?;
    // Recheck a staged download at launch time and follow Steam's later move
    // into content/431960 rather than trusting a potentially stale UI snapshot.
    let wallpaper = Wallpaper::load(&wallpaper.folder)?;
    ensure!(
        ["web", "scene", "video"].contains(&wallpaper.kind.as_str()),
        "Wallpaper type '{}' is not supported",
        wallpaper.kind
    );
    ensure!(
        preview
            || (!output.is_empty()
                && !output.starts_with('-')
                && !output.chars().any(char::is_control)),
        "Choose a valid monitor connector name"
    );
    wallpaper.content_path()?;
    let assets = library::scan(config).assets.context("Wallpaper Engine assets are unavailable. Reconnect your iSCSI library, set its path, and rescan.")?;
    let mut args: Vec<OsString> = vec![
        "--assets-dir".into(),
        assets.into_os_string(),
        "--fps".into(),
        config.fps.to_string().into(),
    ];
    if config.volume == 0 {
        args.push("--silent".into());
    } else {
        args.extend(["--volume".into(), config.volume.to_string().into()]);
    }
    if preview {
        args.extend(["--window".into(), "0x0x1280x720".into()]);
    } else {
        args.extend(["--screen-root".into(), output.into()]);
    }
    args.extend(["--scaling".into(), config.scaling.clone().into()]);
    for (name, value) in properties {
        ensure!(
            wallpaper.properties.contains_key(name),
            "Unknown wallpaper property: {name}"
        );
        ensure!(
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "Invalid wallpaper property name"
        );
        ensure!(!value.contains('\0'), "Property contains a NUL byte");
        ensure!(
            renderer.supports("--set-property"),
            "Renderer cannot override wallpaper properties"
        );
        args.extend(["--set-property".into(), format!("{name}={value}").into()]);
    }
    if preview {
        args.push(wallpaper.folder.as_os_str().into());
    } else {
        args.extend(["--bg".into(), wallpaper.folder.as_os_str().into()]);
    }
    Ok(LaunchPlan {
        executable: renderer.executable.clone(),
        arguments: args,
        wallpaper,
        output: if preview {
            "preview".into()
        } else {
            output.into()
        },
    })
}

impl LaunchPlan {
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        command.args(&self.arguments);
        command
    }
    pub fn display(&self) -> String {
        std::iter::once(self.executable.as_os_str())
            .chain(self.arguments.iter().map(|v| v.as_os_str()))
            .map(|v| format!("{:?}", v.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(" ")
    }
}
