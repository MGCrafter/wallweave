use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum SettingsPage {
    #[default]
    Playback,
    Files,
}

pub(super) struct SettingsUi {
    pub page: SettingsPage,
    pub draft: Config,
    pub pause: AutoPause,
    pub gaming: GamingMode,
}

impl SettingsUi {
    pub fn new(config: &Config, pause: AutoPause, gaming: GamingMode) -> Self {
        Self {
            page: SettingsPage::Playback,
            draft: config.clone(),
            pause,
            gaming,
        }
    }
}

fn path_text(path: &Option<PathBuf>) -> String {
    path.as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

impl App {
    fn settings_changes(&self) -> usize {
        let draft = &self.settings_ui.draft;
        [
            draft.fps != self.config.fps,
            draft.volume != self.config.volume,
            draft.scaling != self.config.scaling,
            draft.libraries != self.config.libraries,
            self.settings_ui.pause != self.auto_pause,
            self.settings_ui.gaming != self.gaming_mode,
            self.renderer_text.trim() != path_text(&self.config.renderer),
            self.assets_text.trim() != path_text(&self.config.assets),
        ]
        .into_iter()
        .filter(|changed| *changed)
        .count()
    }

    fn discard_settings(&mut self) {
        self.settings_ui.draft = self.config.clone();
        self.settings_ui.pause = self.auto_pause;
        self.settings_ui.gaming = self.gaming_mode;
        self.renderer_text = path_text(&self.config.renderer);
        self.assets_text = path_text(&self.config.assets);
    }

    fn save_settings(&mut self, ctx: &egui::Context) {
        let mut next = self.settings_ui.draft.clone();
        next.renderer = (!self.renderer_text.trim().is_empty())
            .then(|| PathBuf::from(self.renderer_text.trim()));
        next.assets =
            (!self.assets_text.trim().is_empty()).then(|| PathBuf::from(self.assets_text.trim()));
        let result = self
            .paths
            .save_preferences(&next)
            .and_then(|()| self.settings_ui.pause.save(&self.paths))
            .and_then(|()| self.settings_ui.gaming.save(&self.paths));
        match result {
            Ok(()) => {
                // Preserve assignments changed by the supervisor while the form was open.
                next.assignments = self
                    .paths
                    .load()
                    .map(|c| c.assignments)
                    .unwrap_or_else(|_| self.config.assignments.clone());
                self.config = next;
                self.auto_pause = self.settings_ui.pause;
                self.gaming_mode = self.settings_ui.gaming;
                self.discard_settings();
                self.message =
                    "Einstellungen gespeichert. Bibliotheken werden neu eingelesen.".into();
                self.error = false;
                self.refresh(ctx);
                if self.status.ok || !self.status.states.is_empty() {
                    self.action(Request::ReloadPlayback, false, ctx);
                }
            }
            Err(error) => {
                self.message = format!("Einstellungen konnten nicht gespeichert werden: {error:#}");
                self.error = true;
            }
        }
    }

    pub(super) fn settings_navigation(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Einstellungen").small().color(MUTED));
        ui.add_space(8.0);
        for (page, icon, label) in [
            (SettingsPage::Playback, Icon::Play, "Wiedergabe"),
            (SettingsPage::Files, Icon::FolderCog, "Dateien & Renderer"),
        ] {
            if theme::nav(
                ui,
                icon,
                label,
                None,
                self.tab == Tab::Setup && self.settings_ui.page == page,
            )
            .clicked()
            {
                self.settings_ui.page = page;
                self.tab = Tab::Setup;
            }
        }
        ui.add_space(24.0);
        if theme::nav(
            ui,
            Icon::Activity,
            "Aktivität & Diagnose",
            None,
            self.tab == Tab::Activity,
        )
        .clicked()
        {
            self.tab = Tab::Activity;
        }
        ui.add_space((ui.available_height() - 152.0).max(20.0));
        theme::section().inner_margin(12.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::icon(ui, Icon::SlidersHorizontal, 20.0, ACCENT);
            ui.label(theme::heading("So läuft dein Desktop.", 14.0));
            ui.label(
                RichText::new("Bild, Ton und Leistung an einem Ort.")
                    .small()
                    .color(MUTED),
            );
        });
    }

    pub(super) fn setup(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("settings_save")
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin {
                top: 16,
                bottom: 0,
                left: 0,
                right: 0,
            }))
            .show_inside(ui, |ui| {
                ui.separator();
                ui.add_space(12.0);
                let changes = self.settings_changes();
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(if changes == 0 {
                            "Alle Änderungen gespeichert".into()
                        } else {
                            format!("{changes} ungespeicherte Änderungen")
                        })
                        .small()
                        .color(if changes == 0 {
                            theme::SUCCESS
                        } else {
                            MUTED
                        }),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_enabled_ui(changes > 0 && !self.busy && !self.scanning, |ui| {
                            if theme::button(
                                ui,
                                Icon::Check,
                                if self.settings_ui.page == SettingsPage::Files {
                                    "Speichern & neu einlesen"
                                } else {
                                    "Änderungen speichern"
                                },
                                true,
                            )
                            .clicked()
                            {
                                self.save_settings(ctx);
                            }
                            if ui.button("Verwerfen").clicked() {
                                self.discard_settings();
                            }
                        });
                    });
                });
                if self.error {
                    ui.colored_label(DANGER, &self.message);
                }
            });
        let files = self.settings_ui.page == SettingsPage::Files;
        ui.label(theme::heading(
            if files {
                "Dateien & Renderer"
            } else {
                "Wiedergabe"
            },
            28.0,
        ));
        ui.label(
            RichText::new(if files {
                "Verbinde deine Sammlung und richte die Wiedergabe ein."
            } else {
                "Deine Wallpaper. So flüssig und leise, wie du möchtest."
            })
            .size(15.0)
            .color(MUTED),
        );
        ui.add_space(20.0);
        egui::ScrollArea::vertical()
            .id_salt(("settings_form", files))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let width = ui.available_width();
                let wide = width >= 1050.0;
                let main_width = if wide { width - 332.0 } else { width };
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 32.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(main_width, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            ui.set_width(main_width);
                            if files {
                                self.settings_files(ui, ctx);
                            } else {
                                self.settings_playback(ui);
                            }
                        },
                    );
                    if wide {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            ui.set_width(300.0);
                            self.settings_context(ui, files, ctx);
                        });
                    }
                });
                if !wide {
                    ui.add_space(20.0);
                    self.settings_context(ui, files, ctx);
                }
            });
    }

    fn settings_playback(&mut self, ui: &mut egui::Ui) {
        theme::section().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::heading("Bild & Ton", 18.0));
            ui.add_space(12.0);
            setting_row(
                ui,
                "Bildrate",
                "Standard für neu angewendete Wallpaper.",
                |ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.settings_ui.draft.fps)
                            .range(1..=240)
                            .suffix(" FPS"),
                    );
                },
            );
            ui.separator();
            setting_row(
                ui,
                "Lautstärke",
                "Für Wallpaper mit Ton. Aktiv beim Anwenden.",
                |ui| {
                    ui.spacing_mut().slider_width = 130.0;
                    theme::slider(
                        ui,
                        egui::Slider::new(&mut self.settings_ui.draft.volume, 0..=100).suffix(" %"),
                    );
                },
            );
            ui.separator();
            setting_row(
                ui,
                "Bildanpassung",
                "Bildanpassung beim nächsten Anwenden.",
                |ui| {
                    let label = match self.settings_ui.draft.scaling.as_str() {
                        "fit" => "Einpassen",
                        "stretch" => "Strecken",
                        "default" => "Standard",
                        _ => "Ausfüllen",
                    };
                    egui::ComboBox::from_id_salt("settings_scaling")
                        .width(140.0)
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for (value, label) in [
                                ("fill", "Ausfüllen"),
                                ("fit", "Einpassen"),
                                ("stretch", "Strecken"),
                                ("default", "Standard"),
                            ] {
                                ui.selectable_value(
                                    &mut self.settings_ui.draft.scaling,
                                    value.into(),
                                    label,
                                );
                            }
                        });
                },
            );
        });
        ui.add_space(20.0);
        theme::section().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::heading("Beim Spielen", 18.0));
            ui.add_space(8.0);
            let mut enabled = self.settings_ui.gaming == GamingMode::Unload;
            if ui.checkbox(&mut enabled, "Grafikspeicher beim Spielen freigeben").changed() {
                self.settings_ui.gaming = if enabled { GamingMode::Unload } else { GamingMode::Off };
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Entlädt alle Desktop-Wallpaper, solange ein Spiel läuft. Nach Spielende starten sie automatisch wieder – mit deinen bisherigen Einstellungen.").color(MUTED));
            ui.add_space(8.0);
            ui.label(RichText::new("Erkennt Steam-Spiele sowie Gamescope- und als Spiel markierte Fenster unter Hyprland. Bleibt auch beim Alt-Tab aktiv. Fenstervorschauen bleiben geöffnet.").small().color(MUTED));
            if self.status.game_active && self.gaming_mode == GamingMode::Unload {
                ui.add_space(8.0);
                theme::status(ui, "Spiel erkannt · Wallpaper entladen", ACCENT);
            }
        });
        ui.add_space(20.0);
        theme::section().show(ui,|ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::heading("Automatisch pausieren",18.0));
            ui.label(RichText::new("Mehr Leistung für das, was du gerade machst.").color(MUTED));
            ui.add_space(8.0);
            let unavailable = self.desktop.name == "Hyprland"
                && self.engine.as_ref().is_ok_and(|engine| !engine.supports_pause_control());
            if unavailable {
                ui.colored_label(DANGER, "Dieser Renderer kann noch nicht sicher pausieren. Die Automatik bleibt ausgeschaltet, damit der Systemton weiterläuft.");
            }
            for (mode,title,description) in [(AutoPause::Focused,"Wenn eine App im Fokus ist","Pausiert auch bei Vollbild und maximierten Fenstern."),
                (AutoPause::Fullscreen,"Nur bei Vollbild oder maximierten Apps","Beim Arbeiten in kleineren Fenstern läuft das Wallpaper weiter."),
                (AutoPause::Off,"Nie automatisch pausieren","Wallpaper laufen unabhängig von anderen Apps.")] {
                let selected=self.settings_ui.pause==mode;
                egui::Frame::new().fill(if selected {theme::ACTIVE} else {PANEL}).stroke(egui::Stroke::new(1.0_f32,if selected {ACCENT} else {BORDER})).corner_radius(6.0).inner_margin(12.0).show(ui,|ui| {
                    ui.set_width(ui.available_width());
                    let r=ui.add_enabled(!unavailable || mode==AutoPause::Off, egui::RadioButton::new(selected,title));
                    if r.clicked() {self.settings_ui.pause=mode;}
                    ui.indent(("pause_hint",mode.label()),|ui| {ui.label(RichText::new(description).size(12.0).color(MUTED));});
                });
            }
            if let Ok(renderer)=&self.engine && !renderer.supports_auto_pause(self.settings_ui.pause) {
                ui.colored_label(DANGER,"Der Renderer unterstützt diese Regel nicht. Er verwendet seine Standard-Vollbildpause.");
            }
        });
    }

    fn settings_files(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        theme::section().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::heading("Steam-Bibliotheken", 18.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            !self.busy,
                            egui::Button::new(theme::button_text(
                                Icon::Plus,
                                "Bibliothek hinzufügen",
                                TEXT,
                            )),
                        )
                        .clicked()
                    {
                        self.choose(FolderKind::DraftLibrary, ctx);
                    }
                });
            });
            ui.add_space(8.0);
            let mut remove = None;
            for (index, path) in self.settings_ui.draft.libraries.iter().enumerate() {
                ui.separator();
                ui.horizontal(|ui| {
                    theme::icon(ui, Icon::Folder, 20.0, ACCENT);
                    ui.allocate_ui_with_layout(
                        egui::vec2((ui.available_width() - 100.0).max(80.0), 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.add(egui::Label::new(path.display().to_string()).truncate())
                                .on_hover_text(path.display().to_string());
                            ui.label(
                                RichText::new(if path.is_dir() {
                                    "Verfügbar"
                                } else {
                                    "Laufwerk nicht verbunden"
                                })
                                .small()
                                .color(if path.is_dir() {
                                    theme::SUCCESS
                                } else {
                                    Color32::from_rgb(227, 190, 130)
                                }),
                            );
                        },
                    );
                    if ui.small_button("Entfernen").clicked() {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = remove {
                self.settings_ui.draft.libraries.remove(index);
            }
            let discovered: Vec<_> = self
                .scan
                .roots
                .iter()
                .filter(|r| !self.settings_ui.draft.libraries.contains(&r.path))
                .collect();
            for root in discovered {
                ui.separator();
                ui.add(
                    egui::Label::new(RichText::new(root.path.display().to_string()).size(13.0))
                        .truncate(),
                )
                .on_hover_text(root.path.display().to_string());
                ui.label(
                    RichText::new(if root.available {
                        "Automatisch erkannt"
                    } else {
                        "Automatisch erkannt · offline"
                    })
                    .small()
                    .color(MUTED),
                );
            }
            if self.settings_ui.draft.libraries.is_empty() && self.scan.roots.is_empty() {
                ui.colored_label(MUTED, "Noch keine Bibliothek verbunden.");
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new("Dateien bleiben in ihren Ordnern. Es wird nichts verschoben.")
                    .small()
                    .color(MUTED),
            );
        });
        ui.add_space(20.0);
        theme::section().show(ui,|ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::heading("Wallpaper Engine Assets",18.0));
            ui.horizontal(|ui| {
                let width=(ui.available_width()-126.0).max(80.0);
                ui.add(egui::TextEdit::singleline(&mut self.assets_text).desired_width(width).hint_text("Automatisch aus Steam-Bibliotheken ermitteln"));
                if ui.add_enabled(!self.busy,egui::Button::new("Auswählen …")).clicked(){self.choose(FolderKind::Assets,ctx);}
            });
            ui.label(RichText::new("Gemeinsame Ressourcen für Szenen und Effekte. Leeres Feld: automatisch suchen.").small().color(MUTED));
            if let Some(path)=&self.scan.assets {
                ui.add(egui::Label::new(RichText::new(format!("Erkannt: {}",path.display())).small().color(theme::SUCCESS)).truncate()).on_hover_text(path.display().to_string());
            } else {ui.colored_label(DANGER,"Assets noch nicht gefunden.");}
        });
        ui.add_space(20.0);
        theme::section().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::heading("Renderer", 18.0));
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 126.0).max(80.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.renderer_text)
                        .desired_width(width)
                        .hint_text("Automatisch nach linux-wallpaperengine suchen"),
                );
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Auswählen …"))
                    .clicked()
                {
                    self.choose(FolderKind::Renderer, ctx);
                }
            });
            match &self.engine {
                Ok(engine) => {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("Erkannt: {}", engine.executable.display()))
                                .small()
                                .color(theme::SUCCESS),
                        )
                        .truncate(),
                    )
                    .on_hover_text(engine.executable.display().to_string());
                }
                Err(error) => {
                    ui.colored_label(DANGER, error);
                }
            }
            ui.hyperlink_to(
                "Installationshilfe",
                "https://github.com/MGCrafter/wallweave/blob/main/docs/INSTALL.de.md",
            );
        });
    }

    fn settings_context(&mut self, ui: &mut egui::Ui, files: bool, ctx: &egui::Context) {
        theme::section().inner_margin(20.0).show(ui,|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui|{theme::icon(ui,if files{Icon::CircleCheck}else{Icon::Monitor},18.0,ACCENT);ui.label(theme::heading(if files{"Deine Einrichtung"}else{"Dein Desktop"},15.0));});
            ui.add_space(12.0);
            if files {
                for (label,ready) in [(if self.workshop.connected {"Steam verbunden"} else {"Steam nicht verbunden"},self.workshop.connected),("Assets gefunden",self.scan.assets.is_some()),("Renderer erkannt",self.engine.is_ok())] {
                    ui.horizontal(|ui|{theme::icon(ui,if ready{Icon::Check}else{Icon::Info},16.0,if ready{theme::SUCCESS}else{MUTED});ui.label(RichText::new(label).size(13.0).color(if ready{TEXT}else{MUTED}));});
                }
                if self.settings_changes()>0 {ui.label(RichText::new("Status der gespeicherten Einrichtung. Neue Pfade werden beim Speichern geprüft.").small().color(MUTED));}
            } else {
                let assignments=self.display_wallpapers();
                for (output,current) in assignments.iter().take(3) {
                    let wallpaper=self.scan.wallpapers.iter().find(|w|w.matches_folder(&current.folder)).cloned();
                    ui.horizontal(|ui|{
                        if let Some(w)=wallpaper {
                            if let Some(path)=&w.preview {self.thumbnail(path,ctx);}
                            let (r,_)=ui.allocate_exact_size(egui::vec2(72.0,42.0),egui::Sense::hover());self.paint_preview(ui,&w,r,true);
                        }
                        ui.vertical(|ui|{ui.label(RichText::new(output).size(12.0).color(ACCENT));ui.add(egui::Label::new(RichText::new(&current.title).size(12.0)).truncate());});
                    });
                }
                let (summary, color) = self.playback_summary();
                theme::status(ui, &summary, color);
                ui.label(RichText::new(&self.desktop.name).color(MUTED));
                ui.label(RichText::new(&self.desktop.integration).small().color(MUTED));
            }
        });
        ui.add_space(20.0);
        for (title, body) in if files {
            vec![
                (
                    "Alles an seinem Platz",
                    "Nicht verbundene Laufwerke bleiben gespeichert. Verbinde das Laufwerk erneut, damit die Wallpaper wieder verfügbar sind.",
                ),
                (
                    "Was macht der Renderer?",
                    "Er spielt deine Wallpaper ab. Wallweave organisiert die Sammlung und steuert die Wiedergabe.",
                ),
                (
                    "Kompatibilität",
                    "Die Unterstützung hängt vom jeweiligen Wallpaper und deinem Renderer ab.",
                ),
            ]
        } else {
            vec![
                (
                    "Automatisch wieder da",
                    "Endet die Pausenbedingung, läuft das Wallpaper weiter. Manuelle Pausen bleiben bestehen.",
                ),
                (
                    "Für jeden Bildschirm",
                    "Unter Hyprland zählt die sichtbare App auf dem jeweiligen Bildschirm.",
                ),
                (
                    "Beim Speichern",
                    "Spielmodus und Pausenregeln werden sofort übernommen. Bildrate, Ton und Bildanpassung gelten beim nächsten Anwenden.",
                ),
            ]
        } {
            ui.label(theme::heading(title, 13.0));
            ui.label(RichText::new(body).size(12.0).color(MUTED));
            ui.add_space(12.0);
        }
    }
}

fn setting_row(ui: &mut egui::Ui, title: &str, help: &str, control: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let label_width = (ui.available_width() - 240.0).max(160.0);
        ui.allocate_ui_with_layout(
            egui::vec2(label_width, 48.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.label(theme::heading(title, 14.0));
                ui.label(RichText::new(help).small().color(MUTED));
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Assignment;

    fn app() -> (tempfile::TempDir, egui::Context, App) {
        let dir = tempfile::tempdir().expect("settings fixture I/O");
        let paths = Paths {
            config: dir.path().join("config.toml"),
            state: dir.path().join("state"),
            runtime: dir.path().join("run"),
        };
        paths
            .save(&Config {
                discover_steam: false,
                ..Default::default()
            })
            .expect("settings fixture I/O");
        AutoPause::Focused
            .save(&paths)
            .expect("existing preference");
        let ctx = egui::Context::default();
        let mut app = App::new(paths, &ctx);
        app.startup_restore = false;
        app.display_picker.dismiss();
        (dir, ctx, app)
    }

    #[test]
    fn pending_settings_do_not_leak_into_other_save_actions_and_can_be_discarded() {
        let (_dir, _ctx, mut app) = app();
        let original = app.config.clone();
        app.settings_ui.draft.fps = 144;
        app.settings_ui.draft.volume = 73;
        app.settings_ui
            .draft
            .libraries
            .push("/offline/library".into());
        app.settings_ui.pause = AutoPause::Off;
        app.settings_ui.gaming = GamingMode::Unload;
        app.renderer_text = "/pending/renderer".into();
        app.assets_text = "/pending/assets".into();
        assert!(app.settings_changes() >= 5);
        assert!(app.save());
        let disk = app.paths.load().expect("settings fixture I/O");
        assert_eq!(disk.fps, original.fps);
        assert_eq!(disk.volume, original.volume);
        assert_eq!(disk.renderer, original.renderer);
        assert_eq!(disk.assets, original.assets);
        assert_eq!(disk.libraries, original.libraries);
        assert_eq!(
            AutoPause::load(&app.paths).expect("settings fixture I/O"),
            app.auto_pause
        );
        app.discard_settings();
        assert_eq!(app.settings_changes(), 0);
        assert_eq!(app.settings_ui.gaming, GamingMode::Off);
        assert_eq!(
            GamingMode::load(&app.paths).expect("gaming preference"),
            GamingMode::Off
        );
        assert_eq!(app.settings_ui.draft.fps, original.fps);
        assert_eq!(app.settings_ui.draft.libraries, original.libraries);
    }

    #[test]
    fn saving_settings_preserves_assignments_written_while_the_form_was_open() {
        let (_dir, ctx, mut app) = app();
        let mut external = app.config.clone();
        external.assignments.insert(
            "DP-2".into(),
            Assignment {
                wallpaper: "/new/wallpaper".into(),
                properties: BTreeMap::from([("speed".into(), "2".into())]),
            },
        );
        app.paths.save(&external).expect("settings fixture I/O");
        app.settings_ui.draft.fps = 144;
        app.settings_ui.draft.volume = 73;
        app.settings_ui.draft.scaling = "fit".into();
        app.settings_ui.pause = AutoPause::Off;
        app.settings_ui.gaming = GamingMode::Unload;
        app.renderer_text = "  /offline/renderer  ".into();
        app.assets_text = " /offline/assets ".into();
        app.save_settings(&ctx);
        assert!(!app.error, "{}", app.message);
        let saved = app.paths.load().expect("settings fixture I/O");
        assert_eq!(
            (saved.fps, saved.volume, saved.scaling.as_str()),
            (144, 73, "fit")
        );
        assert_eq!(saved.renderer, Some("/offline/renderer".into()));
        assert_eq!(
            saved.assignments["DP-2"].wallpaper,
            PathBuf::from("/new/wallpaper")
        );
        assert_eq!(app.config.assignments["DP-2"].properties["speed"], "2");
        assert_eq!(
            AutoPause::load(&app.paths).expect("settings fixture I/O"),
            AutoPause::Off
        );
        assert_eq!(app.settings_changes(), 0);
        assert_eq!(
            GamingMode::load(&app.paths).expect("gaming preference"),
            GamingMode::Unload
        );
    }

    #[test]
    fn invalid_path_keeps_the_draft_and_does_not_commit_preferences() {
        let (_dir, ctx, mut app) = app();
        let original = std::fs::read(&app.paths.config).expect("settings fixture I/O");
        app.renderer_text = "relative/renderer".into();
        app.settings_ui.draft.fps = 120;
        app.settings_ui.pause = AutoPause::Off;
        app.settings_ui.gaming = GamingMode::Unload;
        app.save_settings(&ctx);
        assert!(app.error);
        assert_eq!(
            std::fs::read(&app.paths.config).expect("settings fixture I/O"),
            original
        );
        assert_eq!(
            AutoPause::load(&app.paths).expect("settings fixture I/O"),
            AutoPause::Focused
        );
        assert_eq!(app.settings_ui.draft.fps, 120);
        assert_eq!(
            GamingMode::load(&app.paths).expect("gaming preference"),
            GamingMode::Off
        );
        assert_eq!(app.renderer_text, "relative/renderer");
        assert_ne!(app.config.fps, 120);
    }

    #[test]
    fn settings_navigation_and_save_actions_fit_both_pages_at_supported_sizes() {
        let (_dir, ctx, mut app) = app();
        app.tab = Tab::Setup;
        app.scanning = false;
        app.settings_ui.draft.fps = 90;
        app.settings_ui.draft.libraries.push(PathBuf::from(format!(
            "/media/{}/SteamLibrary",
            "long-path/".repeat(15)
        )));
        for (width, height) in [(960.0, 640.0), (1600.0, 1040.0)] {
            for page in [SettingsPage::Playback, SettingsPage::Files] {
                app.settings_ui.page = page;
                for frame in 0..3 {
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, height),
                            )),
                            ..Default::default()
                        },
                        |ctx| app.draw(ctx),
                    );
                    if frame < 2 {
                        continue;
                    }
                    for shape in &output.shapes {
                        if let egui::Shape::Rect(rect) = &shape.shape
                            && rect.fill == PANEL
                            && rect.rect.left() > 200.0
                            && rect.rect.width() > 150.0
                        {
                            assert!(
                                rect.rect.right() <= width,
                                "settings card extends beyond window: {:?} at {width}",
                                rect.rect
                            );
                        }
                    }
                    for expected in [
                        "Verwerfen",
                        if page == SettingsPage::Files {
                            "Speichern & neu einlesen"
                        } else {
                            "Änderungen speichern"
                        },
                    ] {
                        let text = output
                            .shapes
                            .iter()
                            .find_map(|s| {
                                if let egui::Shape::Text(t) = &s.shape
                                    && t.galley.text().contains(expected)
                                {
                                    Some(t)
                                } else {
                                    None
                                }
                            })
                            .expect("save footer remains present");
                        assert!(
                            text.pos.y > height * 0.5 && text.pos.y + text.galley.size().y < height,
                            "{expected} clipped vertically"
                        );
                        assert!(
                            text.pos.x + text.galley.size().x <= width,
                            "{expected} clipped horizontally"
                        );
                    }
                }
            }
        }
        app.tab = Tab::Activity;
        let _ = ctx.run(Default::default(), |ctx| app.draw(ctx));
        assert_eq!(
            app.settings_ui.draft.fps, 90,
            "navigation preserves pending edits"
        );
    }
}
