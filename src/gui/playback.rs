use super::*;
use std::collections::BTreeSet;

#[derive(Clone)]
pub(super) struct DisplayWallpaper {
    pub folder: PathBuf,
    pub title: String,
    pub state: &'static str,
}

impl App {
    pub(super) fn restore_on_open(&mut self, ctx: &egui::Context) {
        if !self.startup_restore {
            return;
        }
        if self.snapshot.is_some() || self.config.assignments.is_empty() {
            self.startup_restore = false;
            return;
        }
        if self.scanning
            || self.busy
            || self.polling
            || self.engine.is_err()
            || self.scan.assets.is_none()
            || self.monitors.is_empty()
        {
            return;
        }
        self.startup_restore = false;
        self.busy = true;
        self.message = "Restoring saved wallpapers…".into();
        self.error = false;
        let outputs = self
            .monitors
            .iter()
            .map(|monitor| monitor.name.clone())
            .collect::<Vec<_>>();
        let paths = self.paths.clone();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let event = match service::restore_saved(&paths, &outputs) {
                Ok(response) => Event::Response(response),
                Err(error) => {
                    Event::Error(format!("Could not restore saved wallpapers: {error:#}"))
                }
            };
            let _ = sender.send(event);
            ctx.request_repaint();
        });
    }

    pub(super) fn display_wallpaper(&self, output: &str) -> Option<DisplayWallpaper> {
        if output == "preview" {
            return None;
        }
        if let Some(state) = self.status.states.iter().find(|s| s.output == output) {
            return Some(DisplayWallpaper {
                folder: state.wallpaper.clone(),
                title: state.title.clone(),
                state: if state.paused { "Paused" } else { "Playing" },
            });
        }
        let assignment = self.config.assignments.get(output)?;
        let title = self
            .scan
            .wallpapers
            .iter()
            .find(|w| w.matches_folder(&assignment.wallpaper))
            .map(|w| w.title.clone())
            .unwrap_or_else(|| {
                format!(
                    "Unavailable wallpaper ({})",
                    assignment
                        .wallpaper
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                )
            });
        Some(DisplayWallpaper {
            folder: assignment.wallpaper.clone(),
            title,
            state: "Saved · not running",
        })
    }

    pub(super) fn display_wallpapers(&self) -> BTreeMap<String, DisplayWallpaper> {
        let outputs: BTreeSet<_> = self
            .config
            .assignments
            .keys()
            .chain(self.status.states.iter().map(|s| &s.output))
            .collect();
        outputs
            .into_iter()
            .filter_map(|output| self.display_wallpaper(output).map(|w| (output.clone(), w)))
            .collect()
    }

    pub(super) fn select_display_wallpaper(&mut self, ctx: &egui::Context) {
        if let Some(current) = self.display_wallpaper(&self.output)
            && let Some(wallpaper) = self
                .scan
                .wallpapers
                .iter()
                .find(|w| w.matches_folder(&current.folder))
                .cloned()
        {
            self.select(&wallpaper, ctx);
            self.reveal_wallpaper = Some(wallpaper.folder.clone());
        }
    }

    pub(super) fn current_wallpaper_banner(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let current = self.display_wallpaper(&self.output);
        egui::Frame::new()
            .fill(PANEL)
            .inner_margin(10.0)
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if self.output.is_empty() {
                            "Choose a display"
                        } else {
                            &self.output
                        })
                        .strong()
                        .color(ACCENT),
                    );
                    if let Some(current) = &current {
                        ui.label(RichText::new(current.state).small().color(MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let available = self
                                .scan
                                .wallpapers
                                .iter()
                                .any(|w| w.matches_folder(&current.folder));
                            if ui
                                .add_enabled(available, egui::Button::new("Show wallpaper").small())
                                .clicked()
                            {
                                self.select_display_wallpaper(ctx);
                            }
                        });
                    }
                });
                if let Some(current) = &current {
                    ui.add(egui::Label::new(RichText::new(&current.title).strong()).truncate())
                        .on_hover_text(&current.title);
                } else {
                    ui.label(RichText::new("No wallpaper set on this display").color(MUTED));
                }
            });
        ui.add_space(6.0);
    }

    /// Shared by the Apply button, window preview and tile double-click.
    /// Only the supervisor's successful response updates the displayed assignment.
    pub(super) fn playback_request(
        &self,
        wallpaper: &Wallpaper,
        preview: bool,
    ) -> std::result::Result<Request, String> {
        if self.busy {
            return Err("A wallpaper action is already in progress.".into());
        }
        if let Some(reason) = self.blocked(wallpaper) {
            return Err(reason.into());
        }
        if self.scan.assets.is_none() || self.engine.is_err() {
            return Err("Check the renderer and assets in Settings to enable playback.".into());
        }
        if !preview && self.output.trim().is_empty() {
            return Err("Choose a display before applying a wallpaper.".into());
        }
        Ok(Request::Apply {
            target: wallpaper.folder.display().to_string(),
            output: if preview {
                String::new()
            } else {
                self.output.clone()
            },
            preview,
            properties: self.property_values.clone(),
        })
    }

    pub(super) fn apply_wallpaper(
        &mut self,
        wallpaper: &Wallpaper,
        preview: bool,
        ctx: &egui::Context,
    ) {
        if self.busy {
            return;
        }
        self.select(wallpaper, ctx);
        match self.playback_request(wallpaper, preview) {
            Ok(request) => {
                if self.save() {
                    self.message = if preview {
                        "Opening window preview…".into()
                    } else {
                        format!("Applying {} to {}…", wallpaper.title, self.output)
                    };
                    self.action(request, true, ctx);
                }
            }
            Err(error) => {
                if !preview && self.output.trim().is_empty() {
                    self.display_picker.open();
                }
                self.message = error;
                self.error = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Assignment, service::RunState};
    use serde_json::json;

    fn fixture(id: &str) -> Wallpaper {
        Wallpaper {
            id: id.into(),
            title: format!("Wallpaper {id}"),
            kind: "web".into(),
            folder: PathBuf::from(format!("/fixture/steamapps/workshop/content/431960/{id}")),
            file: "index.html".into(),
            preview: None,
            packed: false,
            staged: false,
            properties: BTreeMap::from([(
                "speed".into(),
                json!({"type":"slider","value":1,"min":0,"max":3}),
            )]),
            localization: Default::default(),
        }
    }

    fn app(root: &std::path::Path, ctx: &egui::Context) -> App {
        let paths = Paths {
            config: root.join("config.toml"),
            state: root.join("state"),
            runtime: root.join("run"),
        };
        paths
            .save(&Config {
                discover_steam: false,
                ..Default::default()
            })
            .expect("config");
        let mut app = App::new(paths, ctx);
        app.output = "DP-2".into();
        app.scan.wallpapers = vec![fixture("100"), fixture("200")];
        app.scan.assets = Some(root.to_path_buf());
        app.engine = Ok(Renderer {
            executable: "/fixture/renderer".into(),
            hot_swap: false,
            web_frame_transfer: false,
            supported_flags: vec![],
        });
        app
    }

    #[test]
    fn startup_restore_waits_for_discovery_and_is_disabled_for_snapshots() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = egui::Context::default();
        let mut app = app(dir.path(), &ctx);
        app.config.assignments.insert(
            "DP-2".into(),
            Assignment {
                wallpaper: fixture("100").folder,
                properties: BTreeMap::new(),
            },
        );
        app.startup_restore = true;
        app.scanning = true;
        app.restore_on_open(&ctx);
        assert!(app.startup_restore && !app.busy);
        app.scanning = false;
        app.scan.assets = None;
        app.restore_on_open(&ctx);
        assert!(app.startup_restore && !app.busy);
        app.scan.assets = Some(dir.path().into());
        app.restore_on_open(&ctx);
        assert!(
            app.startup_restore && !app.busy,
            "Wait for display discovery"
        );
        app.snapshot = Some(dir.path().join("snapshot.png"));
        app.restore_on_open(&ctx);
        assert!(!app.startup_restore && !app.busy);
        assert!(!app.paths.socket().exists());
    }

    #[test]
    fn current_wallpapers_prefer_running_state_and_exclude_window_previews() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = egui::Context::default();
        let mut app = app(dir.path(), &ctx);
        let old = fixture("100");
        let current = fixture("200");
        app.config.assignments.insert(
            "DP-2".into(),
            Assignment {
                wallpaper: old.folder,
                properties: BTreeMap::new(),
            },
        );
        assert_eq!(
            app.display_wallpaper("DP-2").expect("saved").state,
            "Saved · not running"
        );
        let state = RunState {
            output: "DP-2".into(),
            title: current.title.clone(),
            wallpaper: current.folder.clone(),
            paused: false,
            pid: 1,
            log: dir.path().join("log"),
        };
        app.status.states.push(state.clone());
        let displayed = app.display_wallpaper("DP-2").expect("playing");
        assert_eq!(displayed.title, current.title);
        assert_eq!(displayed.state, "Playing");
        app.status.states[0].paused = true;
        assert_eq!(
            app.display_wallpaper("DP-2").expect("paused").state,
            "Paused"
        );
        app.status.states.push(RunState {
            output: "preview".into(),
            ..state
        });
        assert_eq!(app.display_wallpapers().len(), 1);
        assert!(app.display_wallpaper("HDMI-A-1").is_none());
    }

    #[test]
    fn choosing_a_display_selects_its_wallpaper_without_applying_or_losing_drafts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = egui::Context::default();
        let mut app = app(dir.path(), &ctx);
        for (output, id) in [("DP-1", "100"), ("DP-2", "200")] {
            app.config.assignments.insert(
                output.into(),
                Assignment {
                    wallpaper: PathBuf::from(format!(
                        "/fixture/steamapps/workshop/downloads/431960/{id}"
                    )),
                    properties: BTreeMap::from([("speed".into(), "2".into())]),
                },
            );
        }
        app.output = "DP-1".into();
        app.select_display_wallpaper(&ctx);
        assert_eq!(app.selected, Some(fixture("100").folder));
        assert_eq!(app.reveal_wallpaper, app.selected);
        assert_eq!(app.property_values["speed"], "2");
        app.property_values.insert("speed".into(), "3".into());
        app.output = "DP-2".into();
        app.select_display_wallpaper(&ctx);
        assert_eq!(app.selected, Some(fixture("200").folder));
        app.output = "DP-1".into();
        app.select_display_wallpaper(&ctx);
        assert_eq!(app.property_values["speed"], "3");
        assert!(!app.busy);
        assert_eq!(app.config.assignments["DP-1"].properties["speed"], "2");
        let mut local = fixture("100");
        local.folder = "/local/100".into();
        assert!(!local.matches_folder(&fixture("100").folder));
        assert!(fixture("100").matches_folder(std::path::Path::new(
            "/fixture/.wallweave/wallpapers/431960/100"
        )));
    }

    #[test]
    fn quick_apply_obeys_readiness_output_and_busy_guards() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = egui::Context::default();
        let mut app = app(dir.path(), &ctx);
        let wallpaper = fixture("100");
        app.property_values.insert("speed".into(), "2.5".into());
        if let Request::Apply {
            output,
            preview,
            properties,
            ..
        } = app.playback_request(&wallpaper, false).expect("ready")
        {
            assert_eq!(output, "DP-2");
            assert!(!preview);
            assert_eq!(properties["speed"], "2.5");
        } else {
            panic!("Expected apply request");
        }
        app.output.clear();
        assert!(app.playback_request(&wallpaper, false).is_err());
        assert!(app.playback_request(&wallpaper, true).is_ok());
        app.output = "DP-2".into();
        app.busy = true;
        assert!(app.playback_request(&wallpaper, false).is_err());
        app.busy = false;
        app.scan.assets = None;
        assert!(app.playback_request(&wallpaper, false).is_err());
        app.scan.assets = Some(dir.path().into());
        let mut unsupported = wallpaper.clone();
        unsupported.kind = "application".into();
        assert!(app.playback_request(&unsupported, false).is_err());
    }

    #[test]
    fn tile_marks_its_current_display_state_and_other_assignments() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = egui::Context::default();
        let mut app = app(dir.path(), &ctx);
        let wallpaper = fixture("100");
        for output in ["DP-1", "DP-2"] {
            app.config.assignments.insert(
                output.into(),
                Assignment {
                    wallpaper: wallpaper.folder.clone(),
                    properties: BTreeMap::new(),
                },
            );
        }
        let mut painted = None;
        for _ in 0..2 {
            painted = Some(ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.tile(ui, &wallpaper, egui::vec2(240.0, 220.0), ctx)
                    });
                },
            ));
        }
        let texts: Vec<_> = painted
            .expect("tile")
            .shapes
            .into_iter()
            .filter_map(|s| match s.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|text| text == "Saved for DP-2"));
        assert!(texts.iter().any(|text| text.contains("Set on DP-1, DP-2")));
        assert!(!app.busy);
    }

    #[test]
    fn tile_double_click_sends_one_apply_to_the_selected_display() {
        use std::{
            io::{BufRead, BufReader, Write},
            os::unix::net::UnixListener,
        };
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = egui::Context::default();
        let mut app = app(dir.path(), &ctx);
        std::fs::create_dir_all(&app.paths.runtime).expect("runtime");
        let listener = UnixListener::bind(app.paths.socket()).expect("socket");
        listener.set_nonblocking(true).expect("nonblocking");
        let (sender, receiver) = mpsc::channel();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut requests = vec![];
            while requests.len() < 2 && Instant::now() < deadline {
                if let Ok((mut stream, _)) = listener.accept() {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .expect("timeout");
                    let mut line = String::new();
                    BufReader::new(&stream)
                        .read_line(&mut line)
                        .expect("request");
                    requests.push(serde_json::from_str::<Request>(&line).expect("request json"));
                    let mut response = serde_json::to_vec(&Response {
                        ok: true,
                        ..Default::default()
                    })
                    .expect("response");
                    response.push(b'\n');
                    stream.write_all(&response).expect("reply");
                } else {
                    thread::sleep(Duration::from_millis(5));
                }
            }
            sender.send(requests).expect("requests");
        });
        let wallpaper = fixture("100");
        app.select(&wallpaper, &ctx);
        app.property_values.insert("speed".into(), "2.5".into());
        let render = |app: &mut App, time, events| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.tile(ui, &wallpaper, egui::vec2(200.0, 195.0), ctx)
                    });
                },
            );
        };
        render(&mut app, 0.0, vec![]);
        let position = egui::pos2(70.0, 70.0);
        for (click, time) in [(0, 0.1), (1, 0.2)] {
            for (pressed, offset) in [(true, 0.0), (false, 0.01)] {
                render(
                    &mut app,
                    time + offset,
                    vec![
                        egui::Event::PointerMoved(position),
                        egui::Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            if click == 0 {
                assert!(!app.busy, "Single click only selects");
            }
        }
        assert!(app.busy, "Double click starts the shared Apply action");
        let requests = receiver
            .recv_timeout(Duration::from_secs(4))
            .expect("captured requests");
        server.join().expect("mock supervisor");
        assert_eq!(requests.len(), 2);
        assert!(matches!(requests[0], Request::Status));
        match &requests[1] {
            Request::Apply {
                target,
                output,
                preview,
                properties,
            } => {
                assert_eq!(target, &wallpaper.folder.display().to_string());
                assert_eq!(output, "DP-2");
                assert!(!preview);
                assert_eq!(properties["speed"], "2.5");
            }
            _ => panic!("Expected one Apply request"),
        }
        assert!(
            app.config.assignments.is_empty(),
            "Assignment changes only after a successful supervisor response"
        );
    }
}
