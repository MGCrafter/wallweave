use crate::{
    config::{AutoPause, Config, GamingMode, Paths},
    desktop::{self, Desktop, Monitor},
    library::{self, Scan, Wallpaper},
    renderer::Renderer,
    service::{self, Request, Response},
};
use anyhow::Result;
use eframe::egui::{self, Color32, RichText};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

#[path = "gui/workshop.rs"]
mod workshop_ui;
use workshop_ui::WorkshopUi;
mod displays;
use displays::DisplayPicker;
mod collection;
use collection::{CollectionUi, Sort};
mod properties;
use properties::PropertiesUi;
mod icons;
mod playback;
mod settings;
mod theme;
use settings::SettingsUi;
use theme::Icon;

// Restrained desktop chrome lets the wallpaper artwork carry the colour.
const ACCENT: Color32 = Color32::from_rgb(130, 185, 255);
const MUTED: Color32 = Color32::from_rgb(164, 177, 195);
const TEXT: Color32 = Color32::from_rgb(242, 245, 250);
const BG: Color32 = Color32::from_rgb(20, 25, 31);
const PANEL: Color32 = Color32::from_rgb(27, 33, 42);
const SURFACE: Color32 = Color32::from_rgb(37, 45, 57);
const BORDER: Color32 = Color32::from_rgb(51, 62, 77);
const DANGER: Color32 = Color32::from_rgb(255, 138, 145);
const DANGER_BG: Color32 = Color32::from_rgb(66, 39, 46);
const THUMBNAIL_LIMIT: usize = 96;
const THUMBNAIL_WORKERS: usize = 4;

/// Red is reserved for unsupported formats and a reproduced failure in the
/// pinned renderer. Ordinary web/SceneScript projects remain unverified.
fn incompatibility(wallpaper: &Wallpaper, renderer: Option<&Renderer>) -> Option<&'static str> {
    if !["web", "scene", "video"].contains(&wallpaper.kind.as_str()) {
        return Some(if wallpaper.kind == "application" {
            "Windows application wallpapers cannot run in Wallweave."
        } else {
            "This wallpaper format is not supported by Wallweave."
        });
    }
    let pinned_parser = renderer.is_some_and(|r| {
        r.executable
            .components()
            .any(|part| part.as_os_str() == "v0.9.4")
    });
    if pinned_parser
        && wallpaper.id == "corsair_o_tron"
        && wallpaper.properties.values().any(|p| {
            p.get("type").and_then(|v| v.as_str()) == Some("slider")
                && p.get("value").is_some_and(|v| v.is_string())
        })
    {
        return Some(
            "Known renderer limitation: CORSAIR-O-Tron's legacy numeric properties fail to load in the bundled v0.9.4 renderer.",
        );
    }
    None
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum SourceFilter {
    #[default]
    All,
    Workshop,
    Included,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum CompatibilityFilter {
    #[default]
    All,
    NoKnownIssue,
    Incompatible,
}

struct Snapshot {
    scan: Scan,
    renderer: std::result::Result<Renderer, String>,
    monitors: Vec<Monitor>,
    desktop: Desktop,
}
enum Event {
    Scanned(Box<Snapshot>),
    Response(Response),
    Polled(Response),
    Error(String),
    Folder(Option<PathBuf>, FolderKind),
    PropertyFile(Box<Wallpaper>, String, Option<PathBuf>),
    Thumbnail(PathBuf, Option<egui::ColorImage>),
}
#[derive(Clone, Copy)]
enum FolderKind {
    Library,
    DraftLibrary,
    Assets,
    Renderer,
}
#[derive(PartialEq, Eq)]
enum Tab {
    Library,
    Workshop,
    Setup,
    Activity,
}

pub struct App {
    paths: Paths,
    config: Config,
    scan: Scan,
    engine: std::result::Result<Renderer, String>,
    desktop: Desktop,
    monitors: Vec<Monitor>,
    selected: Option<PathBuf>,
    reveal_wallpaper: Option<PathBuf>,
    search: String,
    kind_filter: String,
    source_filter: SourceFilter,
    compatibility_filter: CompatibilityFilter,
    collection: CollectionUi,
    tile_width: f32,
    output: String,
    display_picker: DisplayPicker,
    tab: Tab,
    message: String,
    error: bool,
    scanning: bool,
    scan_at: Instant,
    busy: bool,
    startup_restore: bool,
    polling: bool,
    poll_at: Instant,
    sender: Sender<Event>,
    receiver: Receiver<Event>,
    status: Response,
    textures: HashMap<PathBuf, Option<egui::TextureHandle>>,
    thumbnail_pending: HashSet<PathBuf>,
    texture_used: HashMap<PathBuf, u32>,
    property_values: BTreeMap<String, String>,
    property_ui: PropertiesUi,
    renderer_text: String,
    assets_text: String,
    settings_ui: SettingsUi,
    snapshot: Option<PathBuf>,
    snapshot_wallpaper: Option<Wallpaper>,
    snapshot_requested: bool,
    auto_pause: AutoPause,
    gaming_mode: GamingMode,
    frames: u32,
    workshop: WorkshopUi,
}

impl App {
    fn new(paths: Paths, ctx: &egui::Context) -> Self {
        crate::fonts::configure(ctx);
        let (sender, receiver) = mpsc::channel();
        let loaded = paths.load();
        let playback = AutoPause::load(&paths);
        let error = loaded
            .as_ref()
            .err()
            .or(playback.as_ref().err())
            .map(|e| format!("{e:#}"));
        let config = loaded.unwrap_or_default();
        let mut style = (*ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = BG;
        style.visuals.window_fill = BG;
        style.visuals.extreme_bg_color = Color32::from_rgb(19, 26, 35);
        style.visuals.selection.bg_fill = theme::ACTIVE;
        style.visuals.widgets.inactive.bg_fill = BORDER;
        style.visuals.widgets.hovered.bg_fill = ACCENT;
        style.visuals.widgets.active.bg_fill = ACCENT;
        style.visuals.slider_trailing_fill = true;
        style.spacing.slider_rail_height = 4.0;
        style.visuals.selection.stroke.color = ACCENT;
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
        style.visuals.widgets.inactive.weak_bg_fill = SURFACE;
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(56, 65, 78);
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, ACCENT);
        style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, ACCENT);
        style.visuals.hyperlink_color = ACCENT;
        style.visuals.warn_fg_color = Color32::from_rgb(242, 199, 129);
        style.visuals.error_fg_color = DANGER;
        for widget in [
            &mut style.visuals.widgets.noninteractive,
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
            &mut style.visuals.widgets.open,
        ] {
            widget.corner_radius = egui::CornerRadius::same(6);
        }
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style.spacing.interact_size.y = 22.0;
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(23.0, egui::FontFamily::Name("heading".into())),
        );
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        ctx.set_style(style);
        let workshop = WorkshopUi::load(&paths.state);
        let collection = CollectionUi::load(&paths.state);
        let gaming_mode = GamingMode::load(&paths).unwrap_or_default();
        let mut app = Self {
            settings_ui: SettingsUi::new(
                &config,
                playback.as_ref().copied().unwrap_or_default(),
                gaming_mode,
            ),
            renderer_text: config
                .renderer
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            assets_text: config
                .assets
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            paths,
            config,
            scan: Scan::default(),
            engine: Err("Detecting renderer…".into()),
            desktop: Desktop::detect(),
            monitors: vec![],
            selected: None,
            reveal_wallpaper: None,
            search: String::new(),
            kind_filter: String::new(),
            source_filter: SourceFilter::All,
            compatibility_filter: CompatibilityFilter::All,
            collection,
            tile_width: 280.0,
            output: String::new(),
            display_picker: DisplayPicker::default(),
            tab: Tab::Library,
            message: error
                .clone()
                .unwrap_or_else(|| "Checking Steam libraries…".into()),
            error: error.is_some(),
            scanning: false,
            scan_at: Instant::now(),
            busy: false,
            startup_restore: false,
            polling: false,
            poll_at: Instant::now() - Duration::from_secs(3),
            sender,
            receiver,
            status: Response::default(),
            textures: HashMap::new(),
            thumbnail_pending: HashSet::new(),
            texture_used: HashMap::new(),
            property_values: BTreeMap::new(),
            property_ui: PropertiesUi::default(),
            snapshot: None,
            snapshot_wallpaper: None,
            snapshot_requested: false,
            auto_pause: playback.unwrap_or_default(),
            gaming_mode,
            frames: 0,
            workshop,
        };
        app.refresh(ctx);
        app
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        self.scan_at = Instant::now();
        let config = self.config.clone();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let snapshot = Snapshot {
                scan: library::scan(&config),
                renderer: Renderer::detect(&config).map_err(|e| format!("{e:#}")),
                monitors: desktop::monitors(),
                desktop: Desktop::detect(),
            };
            let _ = sender.send(Event::Scanned(Box::new(snapshot)));
            ctx.request_repaint();
        });
    }

    fn save(&mut self) -> bool {
        match self
            .paths
            .save_preferences(&self.config)
            .and_then(|()| self.auto_pause.save(&self.paths))
        {
            Ok(()) => {
                self.message = "Einstellungen gespeichert. Offline-Pfade bleiben erhalten.".into();
                self.error = false;
                true
            }
            Err(e) => {
                self.message = format!("{e:#}");
                self.error = true;
                false
            }
        }
    }

    fn choose(&mut self, kind: FolderKind, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        self.busy = true;
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let dialog = rfd::FileDialog::new();
            let path = match kind {
                FolderKind::Renderer => dialog
                    .set_title("Choose linux-wallpaperengine or its portable launcher")
                    .pick_file(),
                FolderKind::Library | FolderKind::DraftLibrary => dialog
                    .set_title("Choose your Steam library or steamapps folder")
                    .pick_folder(),
                FolderKind::Assets => dialog
                    .set_title("Choose Wallpaper Engine's assets folder")
                    .pick_folder(),
            };
            let _ = sender.send(Event::Folder(path, kind));
            ctx.request_repaint();
        });
    }

    fn action(&mut self, request: Request, start: bool, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        self.busy = true;
        let paths = self.paths.clone();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = (|| -> Result<Response> {
                if start {
                    service::ensure_running(&paths)?;
                }
                service::client(&paths, &request)
            })();
            let event = match result {
                Ok(response) => Event::Response(response),
                Err(e) => Event::Error(format!("{e:#}")),
            };
            let _ = sender.send(event);
            ctx.request_repaint();
        });
    }

    fn selected(&self) -> Option<Wallpaper> {
        self.scan
            .wallpapers
            .iter()
            .find(|w| Some(&w.folder) == self.selected.as_ref())
            .cloned()
    }

    fn select(&mut self, wallpaper: &Wallpaper, ctx: &egui::Context) {
        if self.selected.as_ref() == Some(&wallpaper.folder) {
            return;
        }
        if let Some(previous) = self.selected() {
            self.property_ui.remember(&previous, &self.property_values);
        }
        self.selected = Some(wallpaper.folder.clone());
        self.property_values = self
            .property_ui
            .select(wallpaper, self.config.assignments.get(&self.output));
        if let Some(path) = &wallpaper.preview {
            self.thumbnail(path, ctx);
        }
    }

    fn thumbnail(&mut self, path: &PathBuf, ctx: &egui::Context) {
        self.request_thumbnail(path, None, ctx);
    }

    fn request_thumbnail(&mut self, path: &PathBuf, url: Option<String>, ctx: &egui::Context) {
        self.texture_used.insert(path.clone(), self.frames);
        if self.textures.contains_key(path) || self.thumbnail_pending.len() >= THUMBNAIL_WORKERS {
            return;
        }
        self.thumbnail_pending.insert(path.clone());
        self.textures.insert(path.clone(), None);
        let path = path.clone();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let image = (|| -> Option<egui::ColorImage> {
                if let Some(url) = url {
                    crate::workshop::fetch_preview(&url, &path).ok()?;
                }
                if std::fs::metadata(&path).ok()?.len() > 32 * 1024 * 1024 {
                    return None;
                }
                let mut reader = image::ImageReader::open(&path)
                    .ok()?
                    .with_guessed_format()
                    .ok()?;
                let mut limits = image::Limits::default();
                limits.max_alloc = Some(64 * 1024 * 1024);
                limits.max_image_width = Some(8192);
                limits.max_image_height = Some(8192);
                reader.limits(limits);
                let decoded = reader.decode().ok()?.thumbnail(640, 400).to_rgba8();
                Some(egui::ColorImage::from_rgba_unmultiplied(
                    [decoded.width() as usize, decoded.height() as usize],
                    decoded.as_raw(),
                ))
            })();
            let _ = sender.send(Event::Thumbnail(path, image));
            ctx.request_repaint();
        });
    }

    fn receive(&mut self, ctx: &egui::Context) {
        self.receive_workshop();
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                Event::Scanned(snapshot) => {
                    let snapshot = *snapshot;
                    let moved_selection = self.selected.as_ref().and_then(|selected| {
                        library::installed_folder(selected).and_then(|installed| {
                            snapshot
                                .scan
                                .wallpapers
                                .iter()
                                .find(|w| w.folder == installed)
                                .cloned()
                        })
                    });
                    self.scanning = false;
                    self.scan_at = Instant::now();
                    if self.message == "Checking Steam libraries…" {
                        self.message = format!(
                            "{} Wallpaper verfügbar. Steam-Downloads werden automatisch geprüft.",
                            snapshot.scan.wallpapers.len()
                        );
                    }
                    self.textures.retain(|path, texture| {
                        texture.is_some() || self.thumbnail_pending.contains(path)
                    });
                    self.texture_used
                        .retain(|path, _| self.textures.contains_key(path));
                    self.scan = snapshot.scan;
                    self.engine = snapshot.renderer;
                    self.monitors = snapshot.monitors;
                    self.desktop = snapshot.desktop;
                    if let Some(wallpaper) = moved_selection {
                        let mut properties = self.property_values.clone();
                        properties.retain(|key, _| wallpaper.properties.contains_key(key));
                        self.select(&wallpaper, ctx);
                        self.reveal_wallpaper = Some(wallpaper.folder.clone());
                        self.property_values = properties;
                    }
                    if self.output.is_empty()
                        && let Some(monitor) = desktop::preferred_monitor(&self.monitors)
                    {
                        self.output = monitor.name.clone();
                        self.select_display_wallpaper(ctx);
                    }
                    self.display_picker.monitors_ready(&self.monitors);
                    if self.snapshot.is_some() && matches!(self.tab, Tab::Setup | Tab::Workshop) {
                        self.display_picker.dismiss();
                    }
                    if let Some(wallpaper) = self.snapshot_wallpaper.take() {
                        self.display_picker.dismiss();
                        if !self
                            .scan
                            .wallpapers
                            .iter()
                            .any(|w| w.folder == wallpaper.folder)
                        {
                            self.scan.wallpapers.push(wallpaper.clone());
                        }
                        self.select(&wallpaper, ctx);
                        self.reveal_wallpaper = Some(wallpaper.folder.clone());
                    }
                    if self
                        .selected
                        .as_ref()
                        .is_some_and(|path| !self.scan.wallpapers.iter().any(|w| &w.folder == path))
                    {
                        self.selected = None;
                        self.property_values.clear();
                    }
                }
                Event::Response(response) => {
                    self.busy = false;
                    self.error = !response.ok;
                    self.message = response.message.clone();
                    self.status = response;
                    if let Ok(config) = self.paths.load() {
                        self.config.assignments = config.assignments;
                    }
                }
                Event::Polled(response) => {
                    self.polling = false;
                    self.status = response;
                    if let Ok(config) = self.paths.load() {
                        self.config.assignments = config.assignments;
                    }
                }
                Event::Error(error) => {
                    self.busy = false;
                    self.error = true;
                    self.message = error;
                }
                Event::PropertyFile(wallpaper, key, path) => {
                    self.busy = false;
                    if let Some(path) = path {
                        if self
                            .selected()
                            .is_some_and(|w| w.favorite_key() == wallpaper.favorite_key())
                        {
                            self.property_values.insert(key, path.display().to_string());
                        } else {
                            self.property_ui
                                .set_file(&wallpaper, key, path.display().to_string());
                        }
                    }
                }
                Event::Folder(path, kind) => {
                    self.busy = false;
                    if let Some(path) = path {
                        match kind {
                            FolderKind::Library => {
                                if !self.config.libraries.contains(&path) {
                                    self.config.libraries.push(path.clone());
                                    if !self.settings_ui.draft.libraries.contains(&path) {
                                        self.settings_ui.draft.libraries.push(path);
                                    }
                                }
                                if self.save() {
                                    self.refresh(ctx);
                                }
                            }
                            FolderKind::DraftLibrary => {
                                if !self.settings_ui.draft.libraries.contains(&path) {
                                    self.settings_ui.draft.libraries.push(path);
                                }
                            }
                            FolderKind::Assets => self.assets_text = path.display().to_string(),
                            FolderKind::Renderer => self.renderer_text = path.display().to_string(),
                        }
                    }
                }
                Event::Thumbnail(path, image) => {
                    self.thumbnail_pending.remove(&path);
                    // Visible rows are virtualized; a bounded LRU holds decoded thumbnails.
                    while self.textures.len() > THUMBNAIL_LIMIT {
                        let evict = self
                            .textures
                            .keys()
                            .filter(|key| **key != path && !self.thumbnail_pending.contains(*key))
                            .min_by_key(|key| self.texture_used.get(*key).copied().unwrap_or(0))
                            .cloned();
                        let Some(evict) = evict else { break };
                        self.textures.remove(&evict);
                        self.texture_used.remove(&evict);
                    }
                    let texture = image.map(|image| {
                        ctx.load_texture(
                            path.display().to_string(),
                            image,
                            egui::TextureOptions::LINEAR,
                        )
                    });
                    self.textures.insert(path, texture);
                }
            }
        }
        self.restore_on_open(ctx);
        self.dispatch_workshop_queue();
        if self.snapshot.is_none()
            && !self.scanning
            && !self.collection.attempted
            && self.collection.preferences.sort.uses_subscription_date()
        {
            self.sync_subscription_dates();
        }
        if self.workshop.needs_scan && !self.scanning {
            self.workshop.needs_scan = false;
            self.refresh(ctx);
        }
        if !self.busy
            && !self.scanning
            && self.scan_at.elapsed() >= Duration::from_secs(10)
            && (self.startup_restore
                || self.tab == Tab::Library
                || self.scan.downloads.iter().any(|d| d.pending()))
        {
            self.refresh(ctx);
        }
        if !self.busy && !self.polling && self.poll_at.elapsed() > Duration::from_secs(2) {
            self.polling = true;
            self.poll_at = Instant::now();
            let paths = self.paths.clone();
            let sender = self.sender.clone();
            let ctx = ctx.clone();
            thread::spawn(move || {
                let response =
                    service::client(&paths, &Request::Status).unwrap_or_else(|_| Response {
                        ok: true,
                        message: "No wallpapers running".into(),
                        ..Default::default()
                    });
                let _ = sender.send(Event::Polled(response));
                ctx.request_repaint();
            });
        }
        ctx.request_repaint_after(Duration::from_millis(350));
    }

    fn empty(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(44.0);
        let offline = self.scan.roots.iter().any(|r| !r.available);
        let pending = self.scan.downloads.iter().any(|d| d.pending());
        ui.heading(if pending {
            "Steam bereitet deine Wallpaper vor"
        } else if offline {
            "Deine Bibliothek ist nicht verbunden"
        } else {
            "Deine Wallpaper. Auf deinem Desktop."
        });
        ui.add_space(10.0);
        ui.label(
            RichText::new(if pending {
                "Wallpaper erscheinen, sobald ihr Download abgeschlossen ist."
            } else {
                "Verbinde dein Laufwerk und füge deine Steam-Bibliothek hinzu."
            })
            .size(18.0),
        );
        ui.colored_label(
            MUTED,
            "Gespeicherte Pfade bleiben erhalten, wenn ein Laufwerk nicht verbunden ist.",
        );
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new(RichText::new("Steam-Bibliothek hinzufügen").color(BG))
                        .fill(ACCENT),
                )
                .clicked()
            {
                self.choose(FolderKind::Library, ctx);
            }
            if ui.button("Einstellungen öffnen").clicked() {
                self.tab = Tab::Setup;
            }
        });
        ui.add_space(28.0);
        ui.separator();
        ui.add_space(8.0);
        ui.label("Ein Demo-Wallpaper zum Ausprobieren ist bereits enthalten:");
        ui.monospace("wallweave inspect examples/web-pulse");
        ui.colored_label(MUTED, "Zur Wiedergabe werden der Renderer und die gemeinsamen Wallpaper-Engine-Assets benötigt.");
        for root in &self.scan.roots {
            ui.add_space(8.0);
            ui.label(root.path.display().to_string());
            ui.colored_label(MUTED, &root.message);
        }
    }

    fn blocked(&self, wallpaper: &Wallpaper) -> Option<&'static str> {
        incompatibility(wallpaper, self.engine.as_ref().ok())
    }

    fn filtered(&self) -> Vec<usize> {
        let query = self.search.to_lowercase();
        let mut items: Vec<_> = self
            .scan
            .wallpapers
            .iter()
            .enumerate()
            .filter(|(_, w)| {
                let workshop = w.workshop_id().is_some();
                (w.title.to_lowercase().contains(&query) || w.id.contains(&query))
                    && (!self.collection.preferences.favorites_only
                        || self.collection.is_favorite(w))
                    && (self.kind_filter.is_empty() || w.kind == self.kind_filter)
                    && match self.source_filter {
                        SourceFilter::All => true,
                        SourceFilter::Workshop => workshop,
                        SourceFilter::Included => !workshop,
                    }
                    && match self.compatibility_filter {
                        CompatibilityFilter::All => true,
                        CompatibilityFilter::NoKnownIssue => self.blocked(w).is_none(),
                        CompatibilityFilter::Incompatible => self.blocked(w).is_some(),
                    }
            })
            .map(|(index, _)| index)
            .collect();
        items.sort_by_cached_key(|index| {
            let w = &self.scan.wallpapers[*index];
            let sort = self.collection.preferences.sort;
            let date = self.collection.subscribed_at(w);
            (
                sort.uses_subscription_date() && date.is_none(),
                match sort {
                    Sort::SubscribedNewest => u32::MAX - date.unwrap_or(0),
                    Sort::SubscribedOldest => date.unwrap_or(0),
                    _ => 0,
                },
                if sort == Sort::Type {
                    w.kind.clone()
                } else {
                    String::new()
                },
                w.title.to_lowercase(),
                w.folder.clone(),
            )
        });
        items
    }

    fn filters(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(RichText::new("Deine Sammlung").small().color(MUTED));
        ui.add_space(8.0);
        let favorites = self
            .scan
            .wallpapers
            .iter()
            .filter(|w| self.collection.is_favorite(w))
            .count();
        if theme::nav(
            ui,
            Icon::LayoutGrid,
            "Alle Wallpaper",
            Some(self.scan.wallpapers.len()),
            !self.collection.preferences.favorites_only && self.kind_filter.is_empty(),
        )
        .clicked()
        {
            self.kind_filter.clear();
            let mut p = self.collection.preferences.clone();
            p.favorites_only = false;
            self.save_collection_preferences(p);
        }
        if theme::nav(
            ui,
            Icon::Heart,
            "Favoriten",
            Some(favorites),
            self.collection.preferences.favorites_only,
        )
        .clicked()
        {
            let mut p = self.collection.preferences.clone();
            p.favorites_only = !p.favorites_only;
            self.save_collection_preferences(p);
        }
        if theme::nav(
            ui,
            Icon::Clock3,
            "Zuletzt hinzugefügt",
            None,
            self.collection.preferences.sort == Sort::SubscribedNewest,
        )
        .clicked()
        {
            let mut p = self.collection.preferences.clone();
            p.sort = Sort::SubscribedNewest;
            self.save_collection_preferences(p);
            if !self.collection.attempted {
                self.sync_subscription_dates();
            }
        }
        ui.add_space(22.0);
        ui.label(RichText::new("Wallpaper-Typ").small().color(MUTED));
        for (kind, label, icon) in [
            ("scene", "Szenen", Icon::Sparkles),
            ("video", "Videos", Icon::Film),
            ("web", "Web", Icon::Globe),
            ("application", "Anwendungen", Icon::PanelsTopLeft),
        ] {
            let count = self
                .scan
                .wallpapers
                .iter()
                .filter(|w| w.kind == kind)
                .count();
            if kind == "application" && count == 0 {
                continue;
            }
            if theme::nav(ui, icon, label, Some(count), self.kind_filter == kind).clicked() {
                self.kind_filter = if self.kind_filter == kind {
                    String::new()
                } else {
                    kind.into()
                };
            }
        }
        ui.add_space(22.0);
        ui.label(RichText::new("Quelle").small().color(MUTED));
        for (source, label) in [
            (SourceFilter::All, "Alle Quellen"),
            (SourceFilter::Workshop, "Steam Workshop"),
            (SourceFilter::Included, "Lokal & mitgeliefert"),
        ] {
            ui.selectable_value(&mut self.source_filter, source, label);
        }
        ui.add_space(22.0);
        ui.label(RichText::new("Kompatibilität").small().color(MUTED));
        let label = match self.compatibility_filter {
            CompatibilityFilter::All => "Alle anzeigen",
            CompatibilityFilter::NoKnownIssue => "Kein bekannter Fehler",
            CompatibilityFilter::Incompatible => "Nicht kompatibel",
        };
        egui::ComboBox::from_id_salt("compatibility")
            .width(ui.available_width())
            .selected_text(label)
            .show_ui(ui, |ui| {
                for (value, label) in [
                    (CompatibilityFilter::All, "Alle anzeigen"),
                    (CompatibilityFilter::NoKnownIssue, "Kein bekannter Fehler"),
                    (CompatibilityFilter::Incompatible, "Nicht kompatibel"),
                ] {
                    ui.selectable_value(&mut self.compatibility_filter, value, label);
                }
            });
        ui.add_space(16.0);
        ui.label(
            RichText::new("Die Unterstützung kann je nach Wallpaper und Renderer variieren.")
                .small()
                .color(MUTED),
        );
        ui.add_space((ui.available_height() - 174.0).clamp(24.0, 100.0));
        theme::section().inner_margin(12.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::icon(ui, Icon::FolderOpen, 20.0, ACCENT);
            ui.label(theme::heading(
                "Deine Wallpaper.\nAuf deinem Desktop.",
                14.0,
            ));
            ui.label(
                RichText::new(format!(
                    "{} Wallpaper verfügbar",
                    self.scan.wallpapers.len()
                ))
                .small()
                .color(MUTED),
            );
        });
        ui.add_space(10.0);
        ui.add_enabled_ui(!self.busy, |ui| {
            if theme::wide_button(ui, Icon::FolderPlus, "Ordner hinzufügen", false).clicked() {
                self.choose(FolderKind::Library, ctx);
            }
        });
    }

    fn library(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let content_width = ui.available_width();
        let compact = content_width < 520.0;
        ui.horizontal(|ui| {
            let title_width = content_width - if compact { 56.0 } else { 146.0 };
            ui.allocate_ui_with_layout(
                egui::vec2(title_width, 36.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_width(title_width);
                    ui.add(egui::Label::new(theme::heading("Deine Bibliothek", 28.0)).truncate());
                },
            );
            ui.add_enabled_ui(!self.scanning, |ui| {
                if theme::button(
                    ui,
                    Icon::RefreshCw,
                    if compact { "" } else { "Aktualisieren" },
                    false,
                )
                .on_hover_text("Bibliothek aktualisieren")
                .clicked()
                {
                    self.refresh(ctx);
                }
            });
        });
        ui.label(RichText::new("Ein neuer Blick auf deinen Desktop.").color(MUTED));
        ui.add_space(14.0);
        let pending: Vec<_> = self.scan.downloads.iter().filter(|d| d.pending()).collect();
        if !pending.is_empty() {
            let downloaded: usize = pending.iter().map(|d| d.downloaded).sum();
            let total: usize = pending.iter().map(|d| d.total).sum();
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(ACCENT, format!("Steam-Downloads   {downloaded} / {total}"));
                ui.label(
                    RichText::new("Fertige Wallpaper sind sofort verfügbar.")
                        .small()
                        .color(MUTED),
                );
            });
        }
        let missing: usize = self.scan.downloads.iter().map(|d| d.missing.len()).sum();
        let active = self
            .workshop
            .jobs
            .values()
            .filter(|job| job.active())
            .count();
        if missing > 0 {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(
                    DANGER,
                    format!(
                        "{missing} zuvor heruntergeladene Dateien fehlen in den Steam-Ordnern."
                    ),
                );
                if ui
                    .add_enabled(
                        active == 0 && !self.workshop.connecting,
                        egui::Button::new("Fehlende Downloads wiederherstellen"),
                    )
                    .clicked()
                {
                    self.restore_missing_downloads();
                }
            });
        }
        if active > 0 {
            ui.horizontal_wrapped(|ui| {
                ui.spinner();
                ui.colored_label(
                    ACCENT,
                    format!(
                        "{active} Downloads vorgemerkt · fertige Wallpaper erscheinen automatisch"
                    ),
                );
                if ui.button("Downloads anzeigen").clicked() {
                    self.tab = Tab::Workshop;
                }
            });
        }
        ui.add_space(6.0);
        if self.scan.wallpapers.is_empty() {
            self.empty(ui, ctx);
            return;
        }
        ui.horizontal(|ui| {
            theme::icon(ui, Icon::Search, 18.0, MUTED);
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Wallpaper suchen …")
                    .desired_width(ui.available_width()),
            );
        });
        ui.horizontal_wrapped(|ui| {
            if !compact {
                ui.label(RichText::new("Sortierung").small().color(MUTED));
            }
            let mut sort = self.collection.preferences.sort;
            egui::ComboBox::from_id_salt("sort")
                .selected_text(sort.label())
                .width(if compact { 160.0 } else { 194.0 })
                .show_ui(ui, |ui| {
                    for &(value, label) in Sort::OPTIONS {
                        ui.selectable_value(&mut sort, value, label);
                    }
                });
            if sort != self.collection.preferences.sort {
                let mut preferences = self.collection.preferences.clone();
                preferences.sort = sort;
                self.save_collection_preferences(preferences);
                if sort.uses_subscription_date() && !self.collection.attempted {
                    self.sync_subscription_dates();
                }
            }
            ui.add_space(8.0);
            theme::icon(ui, Icon::Grid2X2, 16.0, MUTED);
            ui.spacing_mut().slider_width = if compact { 64.0 } else { 90.0 };
            theme::slider(
                ui,
                egui::Slider::new(&mut self.tile_width, 180.0..=380.0).show_value(false),
            );
        });
        if self.collection.preferences.sort.uses_subscription_date() {
            ui.horizontal_wrapped(|ui| {
                if self.collection.syncing {
                    ui.spinner();
                    ui.colored_label(MUTED, &self.collection.message);
                } else if ui.button("Steam-Daten abgleichen").clicked() {
                    self.sync_subscription_dates();
                }
                let workshop = self
                    .scan
                    .wallpapers
                    .iter()
                    .filter(|w| w.workshop_id().is_some())
                    .count();
                let dated = self
                    .scan
                    .wallpapers
                    .iter()
                    .filter(|w| self.collection.subscribed_at(w).is_some())
                    .count();
                ui.label(
                    RichText::new(format!("{dated}/{workshop} mit Datum"))
                        .small()
                        .color(MUTED),
                );
            });
        }
        if self.collection.error {
            ui.colored_label(DANGER, &self.collection.message);
        }
        ui.add_space(6.0);
        ui.set_width(content_width);
        self.current_wallpaper_banner(ui, ctx);
        if ui.available_height() > 420.0 && !self.monitors.is_empty() {
            egui::TopBottomPanel::bottom("display_assignments")
                .frame(
                    egui::Frame::new()
                        .fill(BG)
                        .inner_margin(egui::Margin::symmetric(0, 10)),
                )
                .show_inside(ui, |ui| self.display_overview(ui, ctx));
        }
        let reveal = self.reveal_wallpaper.take();
        let items = self.filtered();
        if items.is_empty() {
            ui.add_space(30.0);
            ui.heading("Keine passenden Wallpaper");
            ui.colored_label(
                MUTED,
                "Versuche andere Suchbegriffe oder setze die Filter zurück.",
            );
            if ui.button("Filter zurücksetzen").clicked() {
                self.search.clear();
                self.kind_filter.clear();
                self.source_filter = SourceFilter::All;
                self.compatibility_filter = CompatibilityFilter::All;
                let mut preferences = self.collection.preferences.clone();
                preferences.favorites_only = false;
                self.save_collection_preferences(preferences);
            }
            return;
        }
        if self.selected.is_none() {
            let wallpaper = self.scan.wallpapers[items[0]].clone();
            self.select(&wallpaper, ctx);
        }
        let gap = 16.0;
        let width = content_width - 10.0; // Leave room for the scrollbar.
        let columns = ((width + gap) / (self.tile_width + gap)).floor().max(1.0) as usize;
        let tile_width = (width - gap * (columns - 1) as f32) / columns as f32;
        let tile_height = tile_width * 0.5625 + 78.0;
        let rows = items.len().div_ceil(columns);
        ui.spacing_mut().item_spacing.y = gap;
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt((
                "wallpaper_grid",
                &self.kind_filter,
                self.source_filter as u8,
                self.compatibility_filter as u8,
                &self.search,
                self.collection.preferences.sort as u8,
                self.collection.preferences.favorites_only,
            ))
            .auto_shrink([false, false]);
        if let Some(index) = reveal.and_then(|folder| {
            items
                .iter()
                .position(|&i| self.scan.wallpapers[i].folder == folder)
        }) {
            scroll = scroll.vertical_scroll_offset((index / columns) as f32 * (tile_height + gap));
        }
        scroll.show_rows(ui, tile_height, rows, |ui, rows| {
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            for row in rows {
                ui.horizontal(|ui| {
                    for &index in &items[(row * columns)..((row + 1) * columns).min(items.len())] {
                        let wallpaper = self.scan.wallpapers[index].clone();
                        self.tile(ui, &wallpaper, egui::vec2(tile_width, tile_height), ctx);
                    }
                });
            }
        });
    }

    fn tile(
        &mut self,
        ui: &mut egui::Ui,
        wallpaper: &Wallpaper,
        size: egui::Vec2,
        ctx: &egui::Context,
    ) {
        let selected = self.selected.as_ref() == Some(&wallpaper.folder);
        let blocked = self.blocked(wallpaper);
        let assignments = self.display_wallpapers();
        let outputs: Vec<_> = assignments
            .iter()
            .filter(|(_, current)| wallpaper.matches_folder(&current.folder))
            .map(|(output, _)| output.as_str())
            .collect();
        let assignment_text = if outputs.is_empty() {
            String::new()
        } else {
            format!("Auf {} eingerichtet", outputs.join(", "))
        };
        let response = ui
            .push_id(&wallpaper.folder, |ui| {
                ui.add_sized(
                    size,
                    egui::Button::new("").selected(selected).fill(if selected {
                        theme::ACTIVE
                    } else {
                        PANEL
                    }),
                )
            })
            .inner;
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                selected,
                format!(
                    "{}; {}{}; {}",
                    wallpaper.title,
                    wallpaper.kind,
                    if blocked.is_some() {
                        "; incompatible"
                    } else {
                        ""
                    },
                    assignment_text,
                ),
            )
        });
        let rect = response.rect;
        let image_rect = egui::Rect::from_min_size(
            rect.min + egui::vec2(3.0, 3.0),
            egui::vec2(size.x - 6.0, size.x * 0.5625 - 3.0),
        );
        if ui.is_rect_visible(rect) {
            if let Some(path) = &wallpaper.preview {
                self.thumbnail(path, ctx);
            }
            self.paint_preview(ui, wallpaper, image_rect, true);
            let mut job = egui::text::LayoutJob::simple(
                wallpaper.title.clone(),
                egui::FontId::new(15.0, egui::FontFamily::Name("heading".into())),
                TEXT,
                size.x - 24.0,
            );
            job.wrap.max_rows = 2;
            let galley = ui.fonts(|fonts| fonts.layout_job(job));
            ui.painter().galley(
                egui::pos2(rect.left() + 12.0, image_rect.bottom() + 10.0),
                galley,
                TEXT,
            );
            let mut metadata = egui::text::LayoutJob::simple(
                if outputs.is_empty() {
                    theme::kind(&wallpaper.kind).to_owned()
                } else {
                    format!("{} · {assignment_text}", theme::kind(&wallpaper.kind))
                },
                egui::FontId::proportional(12.0),
                MUTED,
                size.x - 16.0,
            );
            metadata.wrap.max_rows = 1;
            let galley = ui.fonts(|fonts| fonts.layout_job(metadata));
            ui.painter().galley(
                egui::pos2(rect.left() + 12.0, rect.bottom() - 20.0),
                galley,
                MUTED,
            );
            if let Some(current) = assignments
                .get(&self.output)
                .filter(|current| wallpaper.matches_folder(&current.folder))
            {
                let label = if current.state == "Saved · not running" {
                    format!("Gespeichert für {}", self.output)
                } else {
                    format!("{} · {}", theme::playback(current.state), self.output)
                };
                let mut job = egui::text::LayoutJob::simple(
                    label,
                    egui::FontId::proportional(12.0),
                    TEXT,
                    image_rect.width() - 24.0,
                );
                job.wrap.max_rows = 1;
                let galley = ui.fonts(|fonts| fonts.layout_job(job));
                let badge_rect = egui::Rect::from_min_size(
                    image_rect.left_bottom() + egui::vec2(6.0, -galley.size().y - 14.0),
                    galley.size() + egui::vec2(12.0, 8.0),
                );
                ui.painter().rect_filled(badge_rect, 3.0, BG);
                ui.painter().rect_stroke(
                    badge_rect,
                    3.0,
                    egui::Stroke::new(1.0_f32, ACCENT),
                    egui::StrokeKind::Inside,
                );
                ui.painter()
                    .galley(badge_rect.min + egui::vec2(6.0, 4.0), galley, TEXT);
            }
            let badge = if blocked.is_some() {
                Some(("Nicht kompatibel", DANGER, DANGER_BG))
            } else if wallpaper.staged {
                Some(("Download bereit", ACCENT, Color32::from_rgb(32, 49, 68)))
            } else {
                None
            };
            if let Some((label, color, fill)) = badge {
                let galley = ui.painter().layout_no_wrap(
                    label.into(),
                    egui::FontId::proportional(12.0),
                    color,
                );
                let badge_rect = egui::Rect::from_min_size(
                    image_rect.left_top() + egui::vec2(6.0, 6.0),
                    galley.size() + egui::vec2(12.0, 8.0),
                );
                ui.painter().rect_filled(badge_rect, 3.0, fill);
                ui.painter()
                    .galley(badge_rect.min + egui::vec2(6.0, 4.0), galley, color);
            }
            let stroke = if selected || response.has_focus() {
                egui::Stroke::new(2.0_f32, ACCENT)
            } else if blocked.is_some() {
                egui::Stroke::new(1.5_f32, DANGER)
            } else if response.hovered() {
                egui::Stroke::new(1.0_f32, ACCENT)
            } else {
                egui::Stroke::new(1.0_f32, BORDER)
            };
            ui.painter()
                .rect_stroke(rect, 8.0, stroke, egui::StrokeKind::Inside);
        }
        let favorite_rect = egui::Rect::from_min_size(
            image_rect.right_top() + egui::vec2(-36.0, 6.0),
            egui::vec2(30.0, 30.0),
        );
        let favorite = self.favorite_button(ui, wallpaper, favorite_rect);
        if response.clicked() && !favorite.clicked() {
            self.select(wallpaper, ctx);
            if response.double_clicked() {
                self.apply_wallpaper(wallpaper, false, ctx);
            }
        }
        response.on_hover_text(format!(
            "{}\n{}\n{}",
            wallpaper.title,
            blocked.unwrap_or(if wallpaper.staged {
                "Download abgeschlossen · bereit zum Anwenden."
            } else {
                "Anklicken zum Anpassen. Doppelklick zum Anwenden auf den gewählten Bildschirm."
            }),
            assignment_text,
        ));
    }

    fn paint_preview(&self, ui: &egui::Ui, wallpaper: &Wallpaper, rect: egui::Rect, crop: bool) {
        ui.painter()
            .rect_filled(rect, 2.0, Color32::from_rgb(19, 26, 35));
        if let Some(texture) = wallpaper
            .preview
            .as_ref()
            .and_then(|p| self.textures.get(p))
            .and_then(Option::as_ref)
        {
            theme::image(ui, texture, rect, crop);
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if wallpaper
                    .preview
                    .as_ref()
                    .is_some_and(|p| self.thumbnail_pending.contains(p))
                {
                    "Vorschau wird geladen …"
                } else {
                    "Keine Vorschau"
                },
                egui::FontId::proportional(13.0),
                MUTED,
            );
        }
    }

    fn detail(&mut self, ui: &mut egui::Ui, wallpaper: &Wallpaper, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("wallpaper_actions")
            .frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin {
                left: 0,
                right: 0,
                top: 12,
                bottom: 0,
            }))
            .show_inside(ui, |ui| self.wallpaper_actions(ui, wallpaper, ctx));
        egui::ScrollArea::vertical()
            .id_salt(("wallpaper_detail", &wallpaper.folder))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(path) = &wallpaper.preview {
                    self.thumbnail(path, ctx);
                }
                let width = ui.available_width();
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(width, (width * 0.5625).min(220.0)),
                    egui::Sense::hover(),
                );
                self.paint_preview(ui, wallpaper, rect, true);
                ui.add_space(8.0);
                ui.label(theme::heading(&wallpaper.title, 23.0));
                ui.label(
                    RichText::new(format!(
                        "{} · {}",
                        theme::kind(&wallpaper.kind),
                        if wallpaper.workshop_id().is_some() {
                            "Steam Workshop"
                        } else {
                            "Lokal"
                        }
                    ))
                    .small()
                    .color(MUTED),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.property_ui.details, false, "Anpassen");
                    ui.selectable_value(&mut self.property_ui.details, true, "Details");
                });
                ui.separator();
                if self.property_ui.details {
                    ui.add_space(8.0);
                    let favorite = self.collection.is_favorite(wallpaper);
                    if ui
                        .button(if favorite {
                            "Favorit entfernen"
                        } else {
                            "Als Favorit speichern"
                        })
                        .clicked()
                    {
                        self.toggle_favorite(wallpaper);
                    }
                    if let Some(reason) = self.blocked(wallpaper) {
                        ui.colored_label(DANGER, "Nicht kompatibel");
                        ui.label(reason);
                    }
                    ui.label(wallpaper.script_description());
                    if wallpaper.staged {
                        ui.colored_label(ACCENT, "Download abgeschlossen · bereit zum Anwenden");
                    }
                    ui.label(RichText::new("Projektordner").strong());
                    ui.label(wallpaper.folder.display().to_string());
                    ui.label(
                        RichText::new(format!("Projekt-ID: {}", wallpaper.id))
                            .small()
                            .color(MUTED),
                    );
                    ui.collapsing("Bildschirmanschluss manuell eingeben", |ui| {
                        let label = ui.label("Bildschirmanschluss");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.output).hint_text("z. B. DP-1"),
                        )
                        .labelled_by(label.id);
                    });
                } else {
                    self.property_ui
                        .show(ui, wallpaper, &mut self.property_values);
                }
            });
        if let Some((key, directory)) = self.property_ui.file_request.take() {
            self.choose_property_file(wallpaper, key, directory, ctx);
        }
    }

    fn choose_property_file(
        &mut self,
        wallpaper: &Wallpaper,
        key: String,
        directory: bool,
        ctx: &egui::Context,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        let wallpaper = wallpaper.clone();
        thread::spawn(move || {
            let dialog = rfd::FileDialog::new().set_title("Choose wallpaper content");
            let path = if directory {
                dialog.pick_folder()
            } else {
                dialog.pick_file()
            };
            let _ = sender.send(Event::PropertyFile(Box::new(wallpaper), key, path));
            ctx.request_repaint();
        });
    }

    fn wallpaper_actions(&mut self, ui: &mut egui::Ui, wallpaper: &Wallpaper, ctx: &egui::Context) {
        let blocked = self.blocked(wallpaper);
        let playable = blocked.is_none() && self.scan.assets.is_some() && self.engine.is_ok();
        let label = ui.label(RichText::new("Anwenden auf").small().color(MUTED));
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("output")
                .width((ui.available_width() - 100.0).max(80.0))
                .selected_text(if self.output.is_empty() {
                    "Bildschirm auswählen"
                } else {
                    &self.output
                })
                .show_ui(ui, |ui| {
                    for monitor in &self.monitors {
                        ui.selectable_value(
                            &mut self.output,
                            monitor.name.clone(),
                            format!("{}   {} × {}", monitor.name, monitor.width, monitor.height),
                        );
                    }
                })
                .response
                .labelled_by(label.id);
            if ui.button("Wählen …").clicked() {
                self.display_picker.open();
            }
        });
        if ui
            .add_enabled(
                playable && !self.busy && !self.output.is_empty(),
                egui::Button::new(
                    RichText::new(if self.output.is_empty() {
                        "Bildschirm auswählen".into()
                    } else {
                        format!("Auf {} anwenden", self.output)
                    })
                    .strong()
                    .color(BG),
                )
                .fill(ACCENT)
                .min_size(egui::vec2(ui.available_width(), 40.0)),
            )
            .clicked()
        {
            self.apply_wallpaper(wallpaper, false, ctx);
        }
        if ui
            .add_enabled(
                playable && !self.busy,
                egui::Button::new("Fenstervorschau öffnen")
                    .min_size(egui::vec2(ui.available_width(), 30.0)),
            )
            .clicked()
        {
            self.apply_wallpaper(wallpaper, true, ctx);
        }
        if let Some(reason) = blocked {
            ui.label(RichText::new(reason).small().color(DANGER));
        } else if !playable {
            ui.label(
                RichText::new("Prüfe Renderer und Assets in den Einstellungen, um die Wiedergabe zu aktivieren.")
                    .small()
                    .color(MUTED),
            );
        }
        if let Some(state) = self
            .status
            .states
            .iter()
            .find(|s| s.output == self.output)
            .cloned()
        {
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "{}: {}",
                    theme::playback(state.playback_label()),
                    state.title
                ))
                .small()
                .color(MUTED),
            );
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !self.busy && (state.pause_supported || state.paused),
                        egui::Button::new(if state.paused { "Fortsetzen" } else { "Pause" })
                            .small(),
                    )
                    .on_disabled_hover_text("Dieser Renderer unterstützt noch keine sichere Pause.")
                    .clicked()
                {
                    self.action(
                        if state.paused {
                            Request::Resume {
                                output: Some(state.output.clone()),
                            }
                        } else {
                            Request::Pause {
                                output: Some(state.output.clone()),
                            }
                        },
                        false,
                        ctx,
                    );
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Stoppen").small())
                    .clicked()
                {
                    self.action(
                        Request::Stop {
                            output: Some(state.output.clone()),
                        },
                        false,
                        ctx,
                    );
                }
            });
        }
    }

    fn activity(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Activity");
        ui.colored_label(MUTED, "Only wallpaper processes started by Wallweave appear here. Closing this window leaves them running.");
        ui.add_space(14.0);
        let states = self.status.states.clone();
        if states.is_empty() {
            ui.label("No wallpapers running.");
        }
        for state in states {
            ui.group(|ui| {
                ui.label(RichText::new(format!("{} · {}", state.output, state.title)).strong());
                ui.colored_label(
                    MUTED,
                    theme::playback(state.playback_label()),
                );
                if let Some(mode) = state.auto_pause {
                    ui.label(format!("Automatic pause: {}", mode.label()));
                    if mode != AutoPause::Off && state.auto_paused.is_none() && !state.unloaded {
                        ui.colored_label(MUTED, "The renderer pauses and resumes automatically; its current automatic pause state is not reported here.");
                    }
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.busy && (state.pause_supported || state.paused),
                            egui::Button::new(if state.paused { "Fortsetzen" } else { "Pause" }),
                        )
                        .on_disabled_hover_text("Dieser Renderer unterstützt noch keine sichere Pause.")
                        .clicked()
                    {
                        self.action(
                            if state.paused {
                                Request::Resume {
                                    output: Some(state.output.clone()),
                                }
                            } else {
                                Request::Pause {
                                    output: Some(state.output.clone()),
                                }
                            },
                            false,
                            ctx,
                        );
                    }
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Stoppen"))
                        .clicked()
                    {
                        self.action(
                            Request::Stop {
                                output: Some(state.output.clone()),
                            },
                            false,
                            ctx,
                        );
                    }
                });
                ui.collapsing("Renderer-Protokoll", |ui| {
                    ui.monospace(service::tail(&state.log, 5000));
                    ui.monospace(state.log.display().to_string());
                });
            });
        }
        if ui
            .add_enabled(
                !self.busy && !self.status.states.is_empty(),
                egui::Button::new("Alle Wallpaper stoppen"),
            )
            .clicked()
        {
            self.action(Request::Stop { output: None }, false, ctx);
        }
        ui.add_space(16.0);
        for failure in &self.status.failures {
            ui.colored_label(DANGER, failure);
        }
        for warning in &self.scan.warnings {
            ui.label(warning);
        }
    }

    fn draw(&mut self, ctx: &egui::Context) {
        let previous_output = self.output.clone();
        let compact = ctx.screen_rect().width() < 1200.0;
        egui::TopBottomPanel::top("navigation")
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(24, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(if compact { 160.0 } else { 184.0 }, 54.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            theme::icon(ui, Icon::Layers, 28.0, ACCENT);
                            ui.label(theme::heading("wallweave", 24.0));
                        },
                    );
                    for (tab, icon, label) in [
                        (Tab::Library, Icon::PanelsTopLeft, "Bibliothek"),
                        (Tab::Workshop, Icon::Compass, "Workshop"),
                        (Tab::Setup, Icon::Settings2, "Einstellungen"),
                    ] {
                        if theme::tab(
                            ui,
                            icon,
                            label,
                            self.tab == tab || tab == Tab::Setup && self.tab == Tab::Activity,
                        )
                        .clicked()
                        {
                            self.tab = tab;
                            if self.tab == Tab::Workshop {
                                self.connect_workshop();
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::button(ui, Icon::Monitor, "Bildschirme", false)
                            .on_hover_text(format!("Zielbildschirm: {}", self.output))
                            .clicked()
                        {
                            self.display_picker.open();
                        }
                        if !compact {
                            theme::status(
                                ui,
                                if self.workshop.connected {
                                    "Steam verbunden"
                                } else {
                                    "Steam nicht verbunden"
                                },
                                if self.workshop.connected {
                                    theme::SUCCESS
                                } else {
                                    MUTED
                                },
                            );
                        }
                    });
                });
            });
        egui::TopBottomPanel::bottom("feedback")
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(16, 7)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.busy || self.scanning {
                        ui.spinner();
                    }
                    if !compact && !self.error {
                        let (summary, color) = self.playback_summary();
                        theme::status(ui, &summary, color);
                        ui.colored_label(MUTED, &self.desktop.name);
                        ui.add_space(14.0);
                    }
                    let width = (ui.available_width() - 50.0).max(40.0);
                    ui.add_sized(
                        [width, 20.0],
                        egui::Label::new(
                            RichText::new(&self.message).small().color(if self.error {
                                DANGER
                            } else {
                                MUTED
                            }),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&self.message);
                    ui.label(
                        RichText::new(env!("CARGO_PKG_VERSION"))
                            .small()
                            .color(MUTED),
                    );
                });
            });
        let side_width = if compact { 190.0 } else { 220.0 };
        match self.tab {
            Tab::Library => {
                egui::SidePanel::left("filters")
                    .exact_width(side_width)
                    .resizable(false)
                    .frame(
                        egui::Frame::new()
                            .fill(PANEL)
                            .inner_margin(egui::Margin::symmetric(16, 24)),
                    )
                    .show(ctx, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("library_filters_scroll")
                            .show(ui, |ui| self.filters(ui, ctx));
                    });
                egui::SidePanel::right("inspector")
                    .default_width(368.0)
                    .width_range(320.0..=480.0)
                    .frame(egui::Frame::new().fill(PANEL).inner_margin(24.0))
                    .show(ctx, |ui| {
                        if let Some(wallpaper) = self.selected() {
                            self.detail(ui, &wallpaper, ctx);
                        } else {
                            ui.heading("Wähle ein Wallpaper");
                            ui.colored_label(
                                MUTED,
                                "Wähle eine Vorschau, um das Wallpaper anzupassen und anzuwenden.",
                            );
                        }
                    });
            }
            Tab::Workshop => {
                egui::SidePanel::left("workshop_filters")
                    .exact_width(side_width)
                    .resizable(false)
                    .frame(
                        egui::Frame::new()
                            .fill(PANEL)
                            .inner_margin(egui::Margin::symmetric(16, 24)),
                    )
                    .show(ctx, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("workshop_filters_scroll")
                            .show(ui, |ui| self.workshop_filters(ui));
                    });
                egui::SidePanel::right("workshop_inspector")
                    .default_width(368.0)
                    .width_range(320.0..=480.0)
                    .frame(egui::Frame::new().fill(PANEL).inner_margin(24.0))
                    .show(ctx, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("workshop_detail")
                            .show(ui, |ui| self.workshop_detail(ui, ctx));
                    });
            }
            Tab::Setup | Tab::Activity => {
                egui::SidePanel::left("settings_navigation")
                    .exact_width(side_width)
                    .resizable(false)
                    .frame(
                        egui::Frame::new()
                            .fill(PANEL)
                            .inner_margin(egui::Margin::symmetric(16, 24)),
                    )
                    .show(ctx, |ui| {
                        let before = self.settings_ui.page;
                        self.settings_navigation(ui);
                        if before != self.settings_ui.page {
                            self.tab = Tab::Setup;
                        }
                        if self.tab == Tab::Activity
                            && ui.button("Zurück zu Einstellungen").clicked()
                        {
                            self.tab = Tab::Setup;
                        }
                    });
            }
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(
                if self.tab == Tab::Setup && !compact {
                    32.0
                } else {
                    24.0
                },
            ))
            .show(ctx, |ui| match self.tab {
                Tab::Library => self.library(ui, ctx),
                Tab::Workshop => self.workshop_browser(ui, ctx),
                Tab::Setup => self.setup(ui, ctx),
                Tab::Activity => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.activity(ui, ctx));
                }
            });
        let assignments = self.display_wallpapers();
        self.display_picker.previews.clear();
        for (output, current) in &assignments {
            let path = self
                .scan
                .wallpapers
                .iter()
                .find(|w| w.matches_folder(&current.folder))
                .and_then(|w| w.preview.clone());
            if let Some(path) = path {
                self.thumbnail(&path, ctx);
                if let Some(Some(texture)) = self.textures.get(&path) {
                    self.display_picker
                        .previews
                        .insert(output.clone(), texture.clone());
                }
            }
        }
        let chosen = self
            .display_picker
            .show(ctx, &self.monitors, &mut self.output, &assignments);
        if chosen || previous_output != self.output {
            self.select_display_wallpaper(ctx);
            self.message = match self.display_wallpaper(&self.output) {
                Some(current) => format!(
                    "{} · {}: {}",
                    self.output,
                    theme::playback(current.state),
                    current.title
                ),
                None => format!(
                    "Kein Wallpaper auf {}. Doppelklick auf eine Vorschau zum Anwenden.",
                    self.output
                ),
            };
            self.error = false;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.frames == 1 {
            eprintln!("GUI event loop active");
        }
        self.receive(ctx);
        self.draw(ctx);
        self.frames += 1;
        if let Some(path) = &self.snapshot {
            let captured = ctx.input(|input| {
                input.events.iter().find_map(|event| {
                    if let egui::Event::Screenshot { image, .. } = event {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(image) = captured {
                let pixels: Vec<u8> = image
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.to_array())
                    .collect();
                if let Err(error) = image::save_buffer(
                    path,
                    &pixels,
                    image.width() as u32,
                    image.height() as u32,
                    image::ColorType::Rgba8,
                ) {
                    eprintln!("Could not save UI snapshot: {error}");
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if self.frames >= 12
                && !self.scanning
                && self.thumbnail_pending.is_empty()
                && self.display_picker.settled(ctx)
                && !self.workshop.connecting
                && !self.workshop.searching
                && !self.snapshot_requested
            {
                self.snapshot_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
        }
    }
}

pub fn run(paths: Paths, restore_missing: bool) -> Result<()> {
    run_inner(paths, None, None, None, restore_missing, None)
}

/// Capture only this app's rendered UI, without collecting other desktop windows.
pub fn snapshot(
    paths: Paths,
    destination: PathBuf,
    workshop: Option<String>,
    wallpaper: Option<String>,
    settings: bool,
    settings_files: bool,
) -> Result<()> {
    let wallpaper = wallpaper
        .map(|target| library::resolve(&target, &paths.load()?))
        .transpose()?;
    run_inner(
        paths,
        Some(destination),
        workshop,
        wallpaper,
        false,
        if settings_files {
            Some(settings::SettingsPage::Files)
        } else if settings {
            Some(settings::SettingsPage::Playback)
        } else {
            None
        },
    )
}

fn run_inner(
    paths: Paths,
    snapshot: Option<PathBuf>,
    workshop: Option<String>,
    wallpaper: Option<Wallpaper>,
    restore_missing: bool,
    settings: Option<settings::SettingsPage>,
) -> Result<()> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    eprintln!(
        "Wallweave {}: opening GUI (Wayland session: {wayland}, blocking VSync: {})",
        env!("CARGO_PKG_VERSION"),
        !wayland,
    );
    let options = eframe::NativeOptions {
        // EGL's blocking swap interval can stop the event loop when a Wayland
        // compositor hides the window (including during launcher activation).
        // Winit already paces Wayland redraws with compositor frame callbacks.
        // See glutin::surface::SwapInterval's Wayland-specific documentation.
        vsync: !wayland,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 1040.0])
            .with_min_inner_size([960.0, 640.0])
            .with_app_id("io.wallweave.Wallweave"),
        ..Default::default()
    };
    eframe::run_native(
        "Wallweave",
        options,
        Box::new(move |cc| {
            let mut app = App::new(paths, &cc.egui_ctx);
            app.snapshot = snapshot;
            app.snapshot_wallpaper = wallpaper;
            app.startup_restore = app.snapshot.is_none();
            if app.snapshot.is_some() {
                app.workshop.resume_ids.clear();
                if let Some(page) = settings {
                    app.tab = Tab::Setup;
                    app.settings_ui.page = page;
                }
            } else if !app.workshop.resume_ids.is_empty() {
                app.connect_workshop();
            }
            if restore_missing {
                app.restore_missing_downloads();
            }
            if let Some(query) = workshop {
                app.workshop.search.text = query;
                app.tab = Tab::Workshop;
                app.connect_workshop();
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("Open native interface: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_and_setup_views_produce_ui_without_a_display_server() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths {
            config: dir.path().join("config.toml"),
            state: dir.path().join("state"),
            runtime: dir.path().join("run"),
        };
        let ctx = egui::Context::default();
        let mut app = App::new(paths, &ctx);
        for (width, height) in [(960.0, 640.0), (1360.0, 840.0)] {
            for tab in [Tab::Library, Tab::Workshop, Tab::Setup, Tab::Activity] {
                app.tab = tab;
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, height),
                    )),
                    ..Default::default()
                };
                let output = ctx.run(input, |ctx| app.draw(ctx));
                assert!(!output.shapes.is_empty());
            }
        }
    }

    fn fixture(id: &str, kind: &str) -> Wallpaper {
        Wallpaper {
            id: id.into(),
            title: format!("中文 Wallpaper {id}"),
            kind: kind.into(),
            folder: PathBuf::from(if id.parse::<u64>().is_ok() {
                format!("/fixture/steamapps/workshop/content/431960/{id}")
            } else {
                format!("/fixture/{id}")
            }),
            file: "index.html".into(),
            packed: false,
            staged: false,
            preview: None,
            properties: BTreeMap::new(),
            localization: Default::default(),
        }
    }

    #[test]
    fn library_toolbar_and_cards_fit_compact_content_widths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = egui::Context::default();
        let mut app = App::new(
            Paths {
                config: dir.path().join("config.toml"),
                state: dir.path().join("state"),
                runtime: dir.path().join("run"),
            },
            &ctx,
        );
        app.startup_restore = false;
        app.scanning = false;
        app.scan.wallpapers = (100..112)
            .map(|id| fixture(&id.to_string(), "web"))
            .collect();
        app.collection.preferences.sort = Sort::SubscribedNewest;
        app.output = "DP-1".into();
        app.config.assignments.insert(
            "DP-1".into(),
            crate::config::Assignment {
                wallpaper: app.scan.wallpapers[0].folder.clone(),
                properties: BTreeMap::new(),
            },
        );
        for width in [330.0, 390.0, 960.0] {
            for _ in 0..3 {
                let mut used = 0.0;
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            app.library(ui, ctx);
                            used = ui.min_rect().width();
                        });
                    },
                );
                assert!(
                    used <= width,
                    "library content expanded to {used} at {width}"
                );
            }
        }
    }

    #[test]
    fn workshop_results_and_failed_downloads_render_at_supported_sizes() {
        use crate::workshop::{Download, Item, Page, Phase};
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = egui::Context::default();
        let mut app = App::new(
            Paths {
                config: dir.path().join("config.toml"),
                state: dir.path().join("state"),
                runtime: dir.path().join("run"),
            },
            &ctx,
        );
        app.tab = Tab::Workshop;
        app.workshop.connected = true;
        app.workshop.page = Some(Page {
            items: vec![Item {
                id: 123,
                title: "中文 Forest wallpaper with a long title that wraps to several lines".into(),
                description: String::new(),
                tags: vec!["Scene".into(), "Nature".into()],
                preview_url: None,
                bytes: 2048,
                votes: 10,
                installed: false,
                subscribed: false,
                subscribed_at: None,
                nsfw: false,
            }],
            total: 1,
            page: 1,
            has_next: false,
        });
        app.workshop.selected = Some(123);
        app.workshop.jobs.insert(
            123,
            Download {
                id: 123,
                phase: Phase::Failed,
                downloaded: 100,
                total: 1000,
                folder: None,
                error: Some("Steam could not complete this download. Retry.".into()),
            },
        );
        for (width, height) in [(960.0, 640.0), (1360.0, 840.0), (1920.0, 1080.0)] {
            // egui measures scroll content on its initial pass.
            for _ in 0..2 {
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
                assert!(!output.shapes.is_empty());
            }
            assert!(
                app.workshop.session.is_none(),
                "Rendering results must not open a Steam connection"
            );
            assert!(!app.busy, "Browsing must not apply a wallpaper");
        }
    }

    #[test]
    fn incompatible_content_is_distinguished_from_unverified_scripts() {
        assert!(incompatibility(&fixture("app", "application"), None).is_some());
        assert!(incompatibility(&fixture("unknown", "unknown"), None).is_some());
        for kind in ["scene", "web", "video"] {
            assert!(incompatibility(&fixture("untested", kind), None).is_none());
        }
        let mut corsair = fixture("corsair_o_tron", "web");
        corsair.properties.insert(
            "showbottom".into(),
            serde_json::json!({"type":"slider", "value":"150"}),
        );
        let mut renderer = Renderer {
            executable: PathBuf::from("/runtime/v0.9.4/linux-wallpaperengine"),
            hot_swap: true,
            web_frame_transfer: true,
            supported_flags: vec![],
        };
        assert!(incompatibility(&corsair, Some(&renderer)).is_some());
        renderer.executable = PathBuf::from("/other/renderer");
        assert!(incompatibility(&corsair, Some(&renderer)).is_none());
        corsair.properties.get_mut("showbottom").expect("property")["value"] =
            serde_json::json!(150);
        renderer.executable = PathBuf::from("/runtime/v0.9.4/linux-wallpaperengine");
        assert!(incompatibility(&corsair, Some(&renderer)).is_none());
    }

    #[test]
    fn library_filters_combine_search_type_source_and_compatibility() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = egui::Context::default();
        let mut app = App::new(
            Paths {
                config: dir.path().join("config.toml"),
                state: dir.path().join("state"),
                runtime: dir.path().join("run"),
            },
            &ctx,
        );
        app.scan.wallpapers = vec![
            fixture("123456", "scene"),
            fixture("local", "web"),
            fixture("windows", "application"),
        ];
        app.source_filter = SourceFilter::Workshop;
        assert_eq!(app.filtered(), vec![0]);
        app.kind_filter = "web".into();
        assert!(app.filtered().is_empty());
        app.source_filter = SourceFilter::Included;
        assert_eq!(app.filtered(), vec![1]);
        app.kind_filter.clear();
        app.compatibility_filter = CompatibilityFilter::Incompatible;
        assert_eq!(app.filtered(), vec![2]);
        app.search = "missing".into();
        assert!(app.filtered().is_empty());
        app.search = "中文".into();
        assert_eq!(app.filtered(), vec![2]);
        app.compatibility_filter = CompatibilityFilter::NoKnownIssue;
        assert_eq!(app.filtered(), vec![1]);
        for (width, height) in [(960.0, 640.0), (1360.0, 840.0), (1920.0, 1080.0)] {
            app.selected = Some(app.scan.wallpapers[2].folder.clone());
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
            assert!(!output.shapes.is_empty());
            assert!(!app.busy, "Browsing must not start a wallpaper");
        }
    }

    fn collection_app(root: &std::path::Path, ctx: &egui::Context) -> App {
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
        App::new(paths, ctx)
    }

    #[test]
    fn subscription_sort_uses_steam_dates_with_undated_items_last_in_both_directions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = egui::Context::default();
        let mut app = collection_app(dir.path(), &ctx);
        app.scan.wallpapers = vec![
            fixture("100", "scene"),
            fixture("200", "web"),
            fixture("300", "scene"),
            fixture("400", "video"),
            fixture("local", "web"),
        ];
        app.collection.preferences.subscription_dates =
            BTreeMap::from([(100, 300), (200, 100), (300, 100), (400, 0)]);
        app.collection.preferences.sort = Sort::SubscribedNewest;
        assert_eq!(app.filtered(), vec![0, 1, 2, 3, 4]);
        app.collection.preferences.sort = Sort::SubscribedOldest;
        assert_eq!(app.filtered(), vec![1, 2, 0, 3, 4]);
        app.toggle_favorite(&fixture("100", "scene"));
        app.toggle_favorite(&fixture("200", "web"));
        app.collection.preferences.favorites_only = true;
        assert_eq!(app.filtered(), vec![1, 0]);
        app.kind_filter = "web".into();
        assert_eq!(app.filtered(), vec![1]);
        app.search = "100".into();
        assert!(app.filtered().is_empty());
    }

    #[test]
    fn favorites_and_sort_survive_restart_without_saving_pending_settings() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = egui::Context::default();
        let mut app = collection_app(dir.path(), &ctx);
        let original_config = std::fs::read(&app.paths.config).expect("config bytes");
        app.renderer_text = "/unsaved/renderer".into();
        let wallpaper = fixture("100", "scene");
        app.toggle_favorite(&wallpaper);
        let mut preferences = app.collection.preferences.clone();
        preferences.sort = Sort::SubscribedOldest;
        preferences.favorites_only = true;
        preferences.subscription_dates.insert(100, 123);
        assert!(app.save_collection_preferences(preferences));
        let loaded = CollectionUi::load(&app.paths.state);
        assert!(loaded.is_favorite(&wallpaper));
        assert_eq!(loaded.preferences.sort, Sort::SubscribedOldest);
        assert!(loaded.preferences.favorites_only);
        assert_eq!(loaded.subscribed_at(&wallpaper), Some(123));
        assert_eq!(
            std::fs::read(&app.paths.config).expect("config bytes"),
            original_config
        );
        app.toggle_favorite(&wallpaper);
        assert!(!CollectionUi::load(&app.paths.state).is_favorite(&wallpaper));

        // A failed write must not present an unsaved favorite as saved.
        std::fs::remove_dir_all(&app.paths.state).expect("remove state directory");
        std::fs::write(&app.paths.state, "blocked directory").expect("blocked state");
        app.toggle_favorite(&wallpaper);
        assert!(!app.collection.is_favorite(&wallpaper));
        assert!(app.collection.error);
    }

    #[test]
    fn subscription_sync_paginates_and_preserves_the_cache_on_failure() {
        use std::os::unix::fs::PermissionsExt;
        for fail_second_page in [false, true] {
            let dir = tempfile::tempdir().expect("tempdir");
            let ctx = egui::Context::default();
            let mut app = collection_app(dir.path(), &ctx);
            let script = dir.path().join("fake-steam-worker");
            std::fs::write(&script, r##"#!/usr/bin/env python3
import json, pathlib, sys
def emit(value):
    print('wallweave-workshop:' + json.dumps(value), flush=True)
emit({'event': 'connected'})
for line in sys.stdin:
    command = json.loads(line)
    if command['command'] == 'shutdown':
        break
    assert command['command'] == 'search'
    search = command['search']
    assert search['order'] == 'Subscriptions' and search['show_nsfw']
    assert not any(search[k] for k in ['text', 'kind', 'category', 'resolution'])
    assert command['request'] >= 2**63
    page = search['page']
    if page == 2 and pathlib.Path(__file__).with_suffix('.fail').exists():
        emit({'event': 'error', 'request': command['request'], 'message': 'fixture failure'})
    else:
        item = {'id': page * 100, 'title': 'Fixture', 'description': '', 'tags': [], 'preview_url': None, 'bytes': 0, 'votes': 0, 'installed': True, 'subscribed': True, 'subscribed_at': page * 10}
        emit({'event': 'results', 'request': command['request'], 'page': {'items': [item], 'page': page, 'total': 2, 'has_next': page == 1}})
"##).expect("worker");
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
                .expect("executable");
            if fail_second_page {
                std::fs::write(script.with_extension("fail"), "").expect("failure flag");
            }
            let mut preferences = app.collection.preferences.clone();
            preferences.subscription_dates.insert(900, 90);
            assert!(app.save_collection_preferences(preferences));
            app.collection.syncing = true;
            app.collection.attempted = true;
            app.workshop.session = Some(crate::workshop::Session::spawn_executable(
                &app.paths,
                Ok(script),
            ));
            let deadline = Instant::now() + Duration::from_secs(5);
            while app.collection.syncing && Instant::now() < deadline {
                app.receive_workshop();
                thread::sleep(Duration::from_millis(10));
            }
            assert!(!app.collection.syncing, "sync finished within deadline");
            assert_eq!(app.collection.error, fail_second_page);
            let loaded = CollectionUi::load(&app.paths.state);
            assert_eq!(
                loaded.preferences.subscription_dates,
                if fail_second_page {
                    BTreeMap::from([(900, 90)])
                } else {
                    BTreeMap::from([(100, 10), (200, 20)])
                }
            );
            assert!(
                app.workshop.session.is_none(),
                "release the read-only Steam connection after sync"
            );
        }
    }

    #[test]
    fn favorite_star_click_does_not_select_or_apply_the_wallpaper() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = egui::Context::default();
        let mut app = collection_app(dir.path(), &ctx);
        let wallpaper = fixture("100", "scene");
        let mut tile_rect = egui::Rect::NOTHING;
        let input = |events| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input(vec![]), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                tile_rect = ui.available_rect_before_wrap();
                app.tile(ui, &wallpaper, egui::vec2(200.0, 195.0), ctx);
            });
        });
        let star = tile_rect.min + egui::vec2(176.0, 24.0);
        for pressed in [true, false] {
            let _ = ctx.run(
                input(vec![
                    egui::Event::PointerMoved(star),
                    egui::Event::PointerButton {
                        pos: star,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]),
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.tile(ui, &wallpaper, egui::vec2(200.0, 195.0), ctx)
                    });
                },
            );
        }
        assert!(app.collection.is_favorite(&wallpaper));
        // A second quick click belongs to the star and must not quick-apply its tile.
        for pressed in [true, false] {
            let _ = ctx.run(
                input(vec![
                    egui::Event::PointerMoved(star),
                    egui::Event::PointerButton {
                        pos: star,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]),
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.tile(ui, &wallpaper, egui::vec2(200.0, 195.0), ctx)
                    });
                },
            );
        }
        assert!(!app.collection.is_favorite(&wallpaper));
        assert!(app.selected.is_none());
        assert!(!app.busy);
        assert!(app.workshop.session.is_none());
    }
}
