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
                state: state.playback_label(),
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
            .fill(Color32::from_rgb(28, 42, 57))
            .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(51, 77, 105)))
            .inner_margin(12.0)
            .corner_radius(6.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    theme::icon(ui, Icon::Monitor, 18.0, ACCENT);
                    if ui
                        .link(if self.output.is_empty() {
                            "Bildschirm auswählen"
                        } else {
                            &self.output
                        })
                        .clicked()
                    {
                        self.display_picker.open();
                    }
                    if let Some(current) = &current {
                        let width = (ui.available_width() - 150.0).max(40.0);
                        if ui
                            .allocate_ui_with_layout(
                                egui::vec2(width, 20.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.set_width(width);
                                    ui.add(
                                        egui::Label::new(&current.title)
                                            .truncate()
                                            .sense(egui::Sense::click()),
                                    )
                                },
                            )
                            .inner
                            .on_hover_text("Wallpaper in der Bibliothek anzeigen")
                            .clicked()
                        {
                            self.select_display_wallpaper(ctx);
                        }
                        ui.label(
                            RichText::new(theme::playback(current.state))
                                .size(12.0)
                                .color(theme::SUCCESS),
                        );
                    } else {
                        ui.add(
                            egui::Label::new(
                                RichText::new("Noch kein Wallpaper zugewiesen").color(MUTED),
                            )
                            .truncate(),
                        );
                    }
                });
            });
        ui.add_space(10.0);
    }

    pub(super) fn display_overview(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.label(theme::heading("Auf deinen Bildschirmen", 14.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.link("Verwalten").clicked() {
                    self.display_picker.open();
                }
            });
        });
        ui.add_space(8.0);
        let monitors = self.monitors.clone();
        egui::ScrollArea::horizontal()
            .id_salt("display_overview_scroll")
            .max_height(76.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let card_width = if monitors.len() <= 2 {
                        (ui.available_width() - 12.0 * monitors.len().saturating_sub(1) as f32)
                            / monitors.len().max(1) as f32
                    } else {
                        260.0
                    };
                    for monitor in monitors {
                        let current = self.display_wallpaper(&monitor.name);
                        let wallpaper = current
                            .as_ref()
                            .and_then(|c| {
                                self.scan
                                    .wallpapers
                                    .iter()
                                    .find(|w| w.matches_folder(&c.folder))
                            })
                            .cloned();
                        let selected = self.output == monitor.name;
                        let response = ui.add_sized(
                            [card_width.max(210.0), 68.0],
                            egui::Button::new("")
                                .fill(if selected { theme::ACTIVE } else { PANEL })
                                .stroke(egui::Stroke::new(
                                    1.0_f32,
                                    if selected { ACCENT } else { BORDER },
                                )),
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Button,
                                true,
                                selected,
                                format!("Bildschirm {}", monitor.name),
                            )
                        });
                        let r = response.rect;
                        let preview = egui::Rect::from_min_size(
                            r.min + egui::vec2(10.0, 12.0),
                            egui::vec2(76.0, 44.0),
                        );
                        if let Some(wallpaper) = &wallpaper {
                            if let Some(path) = &wallpaper.preview {
                                self.thumbnail(path, ctx);
                            }
                            self.paint_preview(ui, wallpaper, preview, true);
                        } else {
                            theme::paint_icon(
                                ui,
                                egui::Rect::from_center_size(
                                    preview.center(),
                                    egui::vec2(24.0, 24.0),
                                ),
                                Icon::Monitor,
                                MUTED,
                            );
                        }
                        ui.painter().text(
                            r.min + egui::vec2(98.0, 14.0),
                            egui::Align2::LEFT_TOP,
                            &monitor.name,
                            egui::FontId::proportional(12.0),
                            if selected { ACCENT } else { MUTED },
                        );
                        let title = current
                            .as_ref()
                            .map(|c| c.title.as_str())
                            .unwrap_or("Kein Wallpaper");
                        let mut job = egui::text::LayoutJob::simple(
                            title.into(),
                            egui::FontId::proportional(13.0),
                            TEXT,
                            (r.width() - 112.0).max(70.0),
                        );
                        job.wrap.max_rows = 1;
                        let galley = ui.fonts(|f| f.layout_job(job));
                        ui.painter()
                            .galley(r.min + egui::vec2(98.0, 35.0), galley, TEXT);
                        if response
                            .on_hover_text(
                                current
                                    .map(|c| format!("{} · {}", c.title, theme::playback(c.state)))
                                    .unwrap_or_else(|| "Bildschirm auswählen".into()),
                            )
                            .clicked()
                        {
                            self.output = monitor.name;
                            self.select_display_wallpaper(ctx);
                        }
                    }
                });
            });
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
            auto_pause: None,
            auto_paused: None,
            pause_supported: true,
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
        assert!(texts.iter().any(|text| text == "Gespeichert für DP-2"));
        assert!(
            texts
                .iter()
                .any(|text| text.contains("Auf DP-1, DP-2 eingerichtet"))
        );
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
