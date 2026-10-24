use crate::{
    config::{Config, Paths},
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
mod playback;

// Restrained desktop chrome lets the wallpaper artwork carry the colour.
const ACCENT: Color32 = Color32::from_rgb(115, 185, 255);
const MUTED: Color32 = Color32::from_rgb(167, 177, 192);
const TEXT: Color32 = Color32::from_rgb(233, 237, 243);
const BG: Color32 = Color32::from_rgb(32, 35, 41);
const PANEL: Color32 = Color32::from_rgb(37, 41, 48);
const SURFACE: Color32 = Color32::from_rgb(45, 50, 59);
const BORDER: Color32 = Color32::from_rgb(65, 73, 86);
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
    snapshot: Option<PathBuf>,
    snapshot_wallpaper: Option<Wallpaper>,
    snapshot_requested: bool,
    frames: u32,
    workshop: WorkshopUi,
}

impl App {
    fn new(paths: Paths, ctx: &egui::Context) -> Self {
        crate::fonts::configure(ctx);
        let (sender, receiver) = mpsc::channel();
        let loaded = paths.load();
        let error = loaded.as_ref().err().map(|e| format!("{e:#}"));
        let config = loaded.unwrap_or_default();
        let mut style = (*ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = BG;
        style.visuals.window_fill = BG;
        style.visuals.extreme_bg_color = Color32::from_rgb(24, 27, 32);
        style.visuals.selection.bg_fill = Color32::from_rgb(40, 68, 97);
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
            widget.corner_radius = egui::CornerRadius::same(4);
        }
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style.spacing.interact_size.y = 36.0;
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(20.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        ctx.set_style(style);
        let workshop = WorkshopUi::load(&paths.state);
        let collection = CollectionUi::load(&paths.state);
        let mut app = Self {
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
            tile_width: 182.0,
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
        self.config.renderer = (!self.renderer_text.trim().is_empty())
            .then(|| PathBuf::from(self.renderer_text.trim()));
        self.config.assets =
            (!self.assets_text.trim().is_empty()).then(|| PathBuf::from(self.assets_text.trim()));
        match self.paths.save_preferences(&self.config) {
            Ok(()) => {
                self.message = "Settings saved. Offline paths will be kept.".into();
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
                FolderKind::Library => dialog
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
                            "{} wallpaper(s) available. Steam downloads are checked automatically.",
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
                                    self.config.libraries.push(path);
                                }
                            }
                            FolderKind::Assets => self.assets_text = path.display().to_string(),
                            FolderKind::Renderer => self.renderer_text = path.display().to_string(),
                        }
                        if self.save() {
                            self.refresh(ctx);
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
            "Steam is preparing your Workshop wallpapers"
        } else if offline {
            "Your wallpaper library is offline"
        } else {
            "Bring your wallpapers to Linux"
        });
        ui.add_space(10.0);
        ui.label(
            RichText::new(if pending {
                "Wallpapers appear as soon as their download is complete."
            } else {
                "Connect your drive, choose its Steam library, then rescan."
            })
            .size(18.0),
        );
        ui.colored_label(
            MUTED,
            "Saved paths stay in place when a disk is disconnected.",
        );
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new(RichText::new("Add Steam library").color(BG)).fill(ACCENT),
                )
                .clicked()
            {
                self.choose(FolderKind::Library, ctx);
            }
            if ui.button("Open settings").clicked() {
                self.tab = Tab::Setup;
            }
        });
        ui.add_space(28.0);
        ui.separator();
        ui.add_space(8.0);
        ui.label("You can inspect the bundled web demo without the Steam drive:");
        ui.monospace("wallweave inspect examples/web-pulse");
        ui.colored_label(MUTED, "Rendering a real Wallpaper Engine project still requires the selected renderer and shared assets.");
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
        ui.add_space(8.0);
        ui.label(RichText::new("Your collection").strong());
        ui.add_space(8.0);
        for (kind, label) in [
            ("", "All wallpapers"),
            ("scene", "Scenes"),
            ("video", "Videos"),
            ("web", "Web"),
            ("application", "Applications"),
        ] {
            let count = self
                .scan
                .wallpapers
                .iter()
                .filter(|w| kind.is_empty() || w.kind == kind)
                .count();
            if ui
                .add_sized(
                    [ui.available_width(), 34.0],
                    egui::Button::new(format!("{label}   {count}"))
                        .selected(self.kind_filter == kind),
                )
                .clicked()
            {
                self.kind_filter = kind.into();
            }
        }
        ui.add_space(8.0);
        let favorites = self
            .scan
            .wallpapers
            .iter()
            .filter(|w| self.collection.is_favorite(w))
            .count();
        if ui
            .selectable_label(
                self.collection.preferences.favorites_only,
                format!("Favorites   {favorites}"),
            )
            .clicked()
        {
            let mut preferences = self.collection.preferences.clone();
            preferences.favorites_only = !preferences.favorites_only;
            self.save_collection_preferences(preferences);
        }
        ui.add_space(16.0);
        ui.label(RichText::new("Source").strong());
        for (filter, label) in [
            (SourceFilter::All, "All sources"),
            (SourceFilter::Workshop, "Steam Workshop"),
            (SourceFilter::Included, "Included / local"),
        ] {
            ui.selectable_value(&mut self.source_filter, filter, label);
        }
        ui.add_space(16.0);
        ui.label(RichText::new("Compatibility").strong());
        ui.selectable_value(
            &mut self.compatibility_filter,
            CompatibilityFilter::All,
            "Show all",
        );
        ui.selectable_value(
            &mut self.compatibility_filter,
            CompatibilityFilter::NoKnownIssue,
            "No known issue",
        );
        let blocked = self
            .scan
            .wallpapers
            .iter()
            .filter(|w| self.blocked(w).is_some())
            .count();
        ui.selectable_value(
            &mut self.compatibility_filter,
            CompatibilityFilter::Incompatible,
            RichText::new(format!("Incompatible   {blocked}")).color(DANGER),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "Unmarked wallpapers have no known blocker; compatibility may still vary.",
            )
            .small()
            .color(MUTED),
        );
        ui.add_space(16.0);
        ui.separator();
        ui.add_space(8.0);
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new("Add library…").min_size(egui::vec2(ui.available_width(), 34.0)),
            )
            .clicked()
        {
            self.choose(FolderKind::Library, ctx);
        }
        if ui
            .add_enabled(
                !self.scanning,
                egui::Button::new("Rescan").min_size(egui::vec2(ui.available_width(), 34.0)),
            )
            .clicked()
        {
            self.refresh(ctx);
        }
    }

    fn library(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.heading("Installed wallpapers");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(MUTED, format!("{} wallpapers", self.scan.wallpapers.len()));
            });
        });
        self.current_wallpaper_banner(ui, ctx);
        let pending: Vec<_> = self.scan.downloads.iter().filter(|d| d.pending()).collect();
        if !pending.is_empty() {
            let downloaded: usize = pending.iter().map(|d| d.downloaded).sum();
            let total: usize = pending.iter().map(|d| d.total).sum();
            ui.horizontal(|ui| {
                ui.colored_label(ACCENT, format!("Steam downloads   {downloaded} / {total}"));
                ui.label(
                    RichText::new("Finished items are ready to use.")
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
                        "{missing} previously downloaded files are missing from Steam's folders."
                    ),
                );
                if ui
                    .add_enabled(
                        active == 0 && !self.workshop.connecting,
                        egui::Button::new("Restore missing downloads"),
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
                        "{active} downloads queued · completed wallpapers appear automatically"
                    ),
                );
                if ui.button("View download queue").clicked() {
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
            ui.label("Search");
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Title or Workshop ID")
                    .desired_width(ui.available_width()),
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Sort");
            let mut sort = self.collection.preferences.sort;
            egui::ComboBox::from_id_salt("sort")
                .selected_text(sort.label())
                .width(194.0)
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
            ui.label("Size");
            ui.spacing_mut().slider_width = 90.0;
            ui.add(egui::Slider::new(&mut self.tile_width, 150.0..=250.0).show_value(false));
        });
        if self.collection.preferences.sort.uses_subscription_date() {
            ui.horizontal_wrapped(|ui| {
                if self.collection.syncing {
                    ui.spinner();
                    ui.colored_label(MUTED, &self.collection.message);
                } else if ui.button("Sync Steam dates").clicked() {
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
                    RichText::new(format!(
                        "{dated}/{workshop} with a date · undated items last"
                    ))
                    .small()
                    .color(MUTED),
                );
            });
        }
        if self.collection.error {
            ui.colored_label(DANGER, &self.collection.message);
        }
        ui.add_space(6.0);
        let reveal = self.reveal_wallpaper.take();
        let items = self.filtered();
        if items.is_empty() {
            ui.add_space(30.0);
            ui.heading("No matching wallpapers");
            ui.colored_label(MUTED, "Try a different search or reset the filters.");
            if ui.button("Reset filters").clicked() {
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
        let gap = 10.0;
        let width = ui.available_width() - 10.0; // Leave room for the scrollbar.
        let columns = ((width + gap) / (self.tile_width + gap)).floor().max(1.0) as usize;
        let tile_width = (width - gap * (columns - 1) as f32) / columns as f32;
        let tile_height = tile_width * 0.625 + 70.0;
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
            format!("Set on {}", outputs.join(", "))
        };
        let response = ui
            .push_id(&wallpaper.folder, |ui| {
                ui.add_sized(size, egui::Button::new("").selected(selected).fill(SURFACE))
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
            egui::vec2(size.x - 6.0, size.x * 0.625 - 3.0),
        );
        if ui.is_rect_visible(rect) {
            if let Some(path) = &wallpaper.preview {
                self.thumbnail(path, ctx);
            }
            self.paint_preview(ui, wallpaper, image_rect, true);
            let mut job = egui::text::LayoutJob::simple(
                wallpaper.title.clone(),
                egui::FontId::proportional(14.0),
                TEXT,
                size.x - 16.0,
            );
            job.wrap.max_rows = 2;
            let galley = ui.fonts(|fonts| fonts.layout_job(job));
            ui.painter().galley(
                egui::pos2(rect.left() + 8.0, image_rect.bottom() + 7.0),
                galley,
                TEXT,
            );
            let mut metadata = egui::text::LayoutJob::simple(
                if outputs.is_empty() {
                    wallpaper.kind.clone()
                } else {
                    format!("{} · {assignment_text}", wallpaper.kind)
                },
                egui::FontId::proportional(12.0),
                MUTED,
                size.x - 16.0,
            );
            metadata.wrap.max_rows = 1;
            let galley = ui.fonts(|fonts| fonts.layout_job(metadata));
            ui.painter().galley(
                egui::pos2(rect.left() + 8.0, rect.bottom() - 22.0),
                galley,
                MUTED,
            );
            if let Some(current) = assignments
                .get(&self.output)
                .filter(|current| wallpaper.matches_folder(&current.folder))
            {
                let label = if current.state == "Saved · not running" {
                    format!("Saved for {}", self.output)
                } else {
                    format!("{} on {}", current.state, self.output)
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
                Some(("Incompatible", DANGER, DANGER_BG))
            } else if wallpaper.staged {
                Some(("Ready download", ACCENT, Color32::from_rgb(32, 49, 68)))
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
                .rect_stroke(rect, 4.0, stroke, egui::StrokeKind::Inside);
        }
        let favorite_rect = egui::Rect::from_min_size(
            image_rect.right_top() + egui::vec2(-36.0, 6.0),
            egui::vec2(30.0, 30.0),
        );
        let favorite = ui
            .push_id((&wallpaper.folder, "favorite"), |ui| {
                self.favorite_button(ui, wallpaper, favorite_rect)
            })
            .inner;
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
                "Download complete; ready to apply."
            } else {
                "Click to customize. Double-click to apply to the selected display."
            }),
            assignment_text,
        ));
    }

    fn paint_preview(&self, ui: &egui::Ui, wallpaper: &Wallpaper, rect: egui::Rect, crop: bool) {
        ui.painter()
            .rect_filled(rect, 2.0, Color32::from_rgb(24, 27, 32));
        if let Some(texture) = wallpaper
            .preview
            .as_ref()
            .and_then(|p| self.textures.get(p))
            .and_then(Option::as_ref)
        {
            let image_size = texture.size_vec2();
            let image_ratio = image_size.x / image_size.y;
            let target_ratio = rect.width() / rect.height();
            let mut uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
            let mut target = rect;
            if crop {
                if image_ratio > target_ratio {
                    let fraction = target_ratio / image_ratio;
                    uv.min.x = (1.0 - fraction) / 2.0;
                    uv.max.x = 1.0 - uv.min.x;
                } else {
                    let fraction = image_ratio / target_ratio;
                    uv.min.y = (1.0 - fraction) / 2.0;
                    uv.max.y = 1.0 - uv.min.y;
                }
            } else {
                let scale = (rect.width() / image_size.x).min(rect.height() / image_size.y);
                target = egui::Rect::from_center_size(rect.center(), image_size * scale);
            }
            ui.painter().image(texture.id(), target, uv, Color32::WHITE);
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if wallpaper
                    .preview
                    .as_ref()
                    .is_some_and(|p| self.thumbnail_pending.contains(p))
                {
                    "Loading preview…"
                } else {
                    "No preview"
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
                    egui::vec2(width, (width * 0.48).min(156.0)),
                    egui::Sense::hover(),
                );
                self.paint_preview(ui, wallpaper, rect, true);
                ui.add_space(8.0);
                ui.label(RichText::new(&wallpaper.title).size(18.0).strong());
                ui.label(
                    RichText::new(format!("{} wallpaper", wallpaper.kind))
                        .small()
                        .color(MUTED),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.property_ui.details, false, "Customize");
                    ui.selectable_value(&mut self.property_ui.details, true, "Details");
                });
                ui.separator();
                if self.property_ui.details {
                    ui.add_space(8.0);
                    let favorite = self.collection.is_favorite(wallpaper);
                    if ui
                        .button(if favorite {
                            "Remove favorite"
                        } else {
                            "Add favorite"
                        })
                        .clicked()
                    {
                        self.toggle_favorite(wallpaper);
                    }
                    if let Some(reason) = self.blocked(wallpaper) {
                        ui.colored_label(DANGER, "Incompatible");
                        ui.label(reason);
                    }
                    ui.label(wallpaper.script_description());
                    if wallpaper.staged {
                        ui.colored_label(ACCENT, "Download complete · ready to apply");
                    }
                    ui.label(RichText::new("Project location").strong());
                    ui.label(wallpaper.folder.display().to_string());
                    ui.label(
                        RichText::new(format!("Project ID: {}", wallpaper.id))
                            .small()
                            .color(MUTED),
                    );
                    ui.collapsing("Enter display connector manually", |ui| {
                        let label = ui.label("Display connector");
                        ui.add(egui::TextEdit::singleline(&mut self.output).hint_text("e.g. DP-1"))
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
        let label = ui.label(RichText::new("Apply to display").small().color(MUTED));
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("output")
                .width((ui.available_width() - 100.0).max(80.0))
                .selected_text(if self.output.is_empty() {
                    "Choose a display"
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
            if ui.button("Choose…").clicked() {
                self.display_picker.open();
            }
        });
        if ui
            .add_enabled(
                playable && !self.busy && !self.output.is_empty(),
                egui::Button::new(
                    RichText::new(if self.output.is_empty() {
                        "Choose a display".into()
                    } else {
                        format!("Apply to {}", self.output)
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
                egui::Button::new("Open window preview")
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
                RichText::new("Check the renderer and assets in Settings to enable playback.")
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
                    if state.paused { "Paused" } else { "Playing" },
                    state.title
                ))
                .small()
                .color(MUTED),
            );
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !self.busy,
                        egui::Button::new(if state.paused { "Resume" } else { "Pause" }).small(),
                    )
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
                    .add_enabled(!self.busy, egui::Button::new("Stop").small())
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

    fn setup(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Settings");
        ui.label("Your files stay in their existing library. Nothing is moved or imported.");
        ui.add_space(16.0);
        ui.label(RichText::new("Steam libraries").strong());
        let mut remove = None;
        for (index, path) in self.config.libraries.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.monospace(path.display().to_string());
                if ui.small_button("Remove").clicked() {
                    remove = Some(index);
                }
            });
            if !path.is_dir() {
                ui.colored_label(
                    MUTED,
                    "Currently unavailable — reconnect the drive when you are ready.",
                );
            }
        }
        if let Some(index) = remove {
            self.config.libraries.remove(index);
        }
        if ui
            .add_enabled(!self.busy, egui::Button::new("Add Steam library…"))
            .clicked()
        {
            self.choose(FolderKind::Library, ctx);
        }
        ui.add_space(14.0);
        ui.label("Wallpaper Engine assets folder");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.assets_text)
                    .hint_text("Automatic discovery from Steam libraries")
                    .desired_width(ui.available_width() - 100.0),
            );
            if ui.button("Browse…").clicked() {
                self.choose(FolderKind::Assets, ctx);
            }
        });
        if let Some(path) = &self.scan.assets {
            ui.colored_label(MUTED, format!("Available: {}", path.display()));
        }
        ui.add_space(14.0);
        ui.label("Renderer executable");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.renderer_text)
                    .hint_text("Bundled portable renderer, then linux-wallpaperengine in PATH")
                    .desired_width(ui.available_width() - 100.0),
            );
            if ui.button("Browse…").clicked() {
                self.choose(FolderKind::Renderer, ctx);
            }
        });
        match &self.engine {
            Ok(engine) => {
                ui.colored_label(MUTED, engine.executable.display().to_string());
                ui.label(if engine.web_frame_transfer { "Extended renderer flags detected; actual wallpaper compatibility still needs testing." } else { "Basic renderer detected. For the V8 fork, use the portable renderer installer supplied with this project." });
            }
            Err(error) => {
                ui.colored_label(DANGER, error);
            }
        }
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.label("FPS limit");
            ui.add(egui::Slider::new(&mut self.config.fps, 1..=120));
            ui.label("Volume");
            ui.add(egui::Slider::new(&mut self.config.volume, 0..=100));
        });
        egui::ComboBox::from_label("Scaling")
            .selected_text(&self.config.scaling)
            .show_ui(ui, |ui| {
                for option in ["fill", "fit", "stretch", "default"] {
                    ui.selectable_value(&mut self.config.scaling, option.into(), option);
                }
            });
        ui.add_space(12.0);
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new(RichText::new("Save settings and rescan").color(BG)).fill(ACCENT),
            )
            .clicked()
            && self.save()
        {
            self.refresh(ctx);
        }
        ui.add_space(22.0);
        ui.separator();
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!(
                "{} · {}",
                self.desktop.name, self.desktop.integration
            ))
            .strong(),
        );
        ui.label(&self.desktop.note);
        ui.colored_label(MUTED, "Hyprland monitor names are detected automatically. Other desktops can use manual connector names. Their runtime integrations still need desktop-specific testing.");
        ui.add_space(8.0);
        ui.monospace(self.paths.config.display().to_string());
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
                    if state.paused {
                        "Paused"
                    } else {
                        "Renderer process running — visual compatibility is not verified"
                    },
                );
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.busy,
                            egui::Button::new(if state.paused { "Resume" } else { "Pause" }),
                        )
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
                        .add_enabled(!self.busy, egui::Button::new("Stop"))
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
                ui.collapsing("Renderer log", |ui| {
                    ui.monospace(service::tail(&state.log, 5000));
                    ui.monospace(state.log.display().to_string());
                });
            });
        }
        if ui
            .add_enabled(
                !self.busy && !self.status.states.is_empty(),
                egui::Button::new("Stop all Wallweave wallpapers"),
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
        egui::TopBottomPanel::top("navigation")
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(18, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("wallweave").size(23.0).strong());
                    ui.add_space(22.0);
                    for (tab, text) in [
                        (Tab::Library, "Installed"),
                        (Tab::Workshop, "Workshop"),
                        (Tab::Setup, "Settings"),
                        (Tab::Activity, "Activity"),
                    ] {
                        if ui
                            .add(
                                egui::Button::new(text)
                                    .selected(self.tab == tab)
                                    .min_size(egui::vec2(84.0, 34.0)),
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
                        if ui
                            .button(format!(
                                "Display: {}",
                                if self.output.is_empty() {
                                    "Choose…"
                                } else {
                                    &self.output
                                }
                            ))
                            .clicked()
                        {
                            self.display_picker.open();
                        }
                        ui.label(
                            RichText::new(format!("{} active", self.status.states.len()))
                                .color(ACCENT),
                        );
                        ui.colored_label(MUTED, &self.desktop.name);
                    });
                });
            });
        egui::TopBottomPanel::bottom("feedback")
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.busy || self.scanning {
                        ui.spinner();
                    }
                    ui.add(
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
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(env!("CARGO_PKG_VERSION"))
                                .small()
                                .color(MUTED),
                        );
                    });
                });
            });
        if self.tab == Tab::Library {
            egui::SidePanel::left("filters")
                .exact_width(176.0)
                .resizable(false)
                .frame(egui::Frame::new().fill(PANEL).inner_margin(12.0))
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.filters(ui, ctx));
                });
            egui::SidePanel::right("inspector").default_width(364.0).width_range(320.0..=480.0)
                .frame(egui::Frame::new().fill(PANEL).inner_margin(14.0)).show(ctx, |ui| {
                    if let Some(wallpaper) = self.selected() { self.detail(ui, &wallpaper, ctx); }
                    else {
                        ui.add_space(20.0);
                        ui.heading("Choose a wallpaper");
                        ui.colored_label(MUTED, "Select a thumbnail to customize its settings and apply it to your display.");
                    }
                });
        } else if self.tab == Tab::Workshop {
            egui::SidePanel::left("workshop_filters")
                .exact_width(176.0)
                .resizable(false)
                .frame(egui::Frame::new().fill(PANEL).inner_margin(12.0))
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.workshop_filters(ui));
                });
            egui::SidePanel::right("workshop_inspector")
                .default_width(316.0)
                .width_range(288.0..=420.0)
                .frame(egui::Frame::new().fill(PANEL).inner_margin(14.0))
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("workshop_detail")
                        .show(ui, |ui| self.workshop_detail(ui, ctx));
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(16.0))
            .show(ctx, |ui| match self.tab {
                Tab::Library => self.library(ui, ctx),
                Tab::Workshop => self.workshop_browser(ui, ctx),
                Tab::Setup => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.setup(ui, ctx));
                }
                Tab::Activity => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.activity(ui, ctx));
                }
            });
        let assignments = self.display_wallpapers();
        let chosen = self
            .display_picker
            .show(ctx, &self.monitors, &mut self.output, &assignments);
        if chosen || previous_output != self.output {
            self.select_display_wallpaper(ctx);
            self.message = match self.display_wallpaper(&self.output) {
                Some(current) => format!("{} · {}: {}", self.output, current.state, current.title),
                None => format!(
                    "No wallpaper set on {}. Double-click a wallpaper to apply it.",
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
    run_inner(paths, None, None, None, restore_missing)
}

/// Capture only this app's rendered UI, without collecting other desktop windows.
pub fn snapshot(
    paths: Paths,
    destination: PathBuf,
    workshop: Option<String>,
    wallpaper: Option<String>,
) -> Result<()> {
    let wallpaper = wallpaper
        .map(|target| library::resolve(&target, &paths.load()?))
        .transpose()?;
    run_inner(paths, Some(destination), workshop, wallpaper, false)
}

fn run_inner(
    paths: Paths,
    snapshot: Option<PathBuf>,
    workshop: Option<String>,
    wallpaper: Option<Wallpaper>,
    restore_missing: bool,
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
            .with_inner_size([1360.0, 840.0])
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
