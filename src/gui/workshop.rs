use super::*;
use crate::workshop::{
    self, Command, Download, Event as SteamEvent, Item, Order, Page, Phase, Search, Session,
};

pub(super) struct WorkshopUi {
    pub session: Option<Session>,
    pub connected: bool,
    pub connecting: bool,
    pub searching: bool,
    pub search: Search,
    pub page: Option<Page>,
    pub selected: Option<u64>,
    pub request: u64,
    pub message: String,
    pub error: bool,
    pub jobs: BTreeMap<u64, Download>,
    pub titles: BTreeMap<u64, String>,
    pub needs_scan: bool,
    pub restore_requested: bool,
    pub resume_ids: Vec<u64>,
}

impl Default for WorkshopUi {
    fn default() -> Self {
        Self {
            session: None,
            connected: false,
            connecting: false,
            searching: false,
            search: Search::default(),
            page: None,
            selected: None,
            request: 0,
            message: "Connect to the running Steam client to browse wallpapers.".into(),
            error: false,
            jobs: BTreeMap::new(),
            titles: BTreeMap::new(),
            needs_scan: false,
            restore_requested: false,
            resume_ids: Vec::new(),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Preferences {
    order: Order,
    show_nsfw: bool,
}

impl WorkshopUi {
    pub(super) fn load(state: &std::path::Path) -> Self {
        let mut ui = Self::default();
        if let Ok(file) = std::fs::read(state.join("workshop-preferences.json"))
            && let Ok(prefs) = serde_json::from_slice::<Preferences>(&file)
        {
            ui.search.order = prefs.order;
            ui.search.show_nsfw = prefs.show_nsfw;
        }
        if let Ok(file) = std::fs::read(state.join("workshop-download-queue.json"))
            && let Ok(ids) = serde_json::from_slice::<Vec<u64>>(&file)
        {
            ui.resume_ids = ids;
        }
        ui
    }
}

fn phase_label(phase: &Phase) -> &'static str {
    match phase {
        Phase::Waiting => "Waiting in Wallweave",
        Phase::Checking => "Checking wallpaper…",
        Phase::Subscribing => "Subscribing in Steam…",
        Phase::Queued => "Queued in Steam",
        Phase::Downloading => "Downloading",
        Phase::Installing => "Steam is finishing installation…",
        Phase::Complete => "Download complete",
        Phase::Failed => "Download failed",
    }
}

fn bytes(value: u64) -> String {
    if value >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", value as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if value >= 1024 * 1024 {
        format!("{:.1} MB", value as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", value / 1024)
    }
}

impl App {
    fn save_workshop_queue(&mut self) {
        let ids: Vec<_> = self
            .workshop
            .jobs
            .values()
            .filter(|job| job.active())
            .map(|job| job.id)
            .collect();
        if let Err(error) = self.write_workshop_state("workshop-download-queue.json", &ids) {
            self.message = format!("Could not save download queue: {error:#}");
            self.error = true;
        }
    }

    pub(super) fn write_workshop_state(
        &self,
        name: &str,
        value: &impl serde::Serialize,
    ) -> anyhow::Result<()> {
        use std::io::Write;
        std::fs::create_dir_all(&self.paths.state)?;
        let mut file = tempfile::NamedTempFile::new_in(&self.paths.state)?;
        serde_json::to_writer(&mut file, value)?;
        file.flush()?;
        file.persist(self.paths.state.join(name))
            .map_err(|e| e.error)?;
        Ok(())
    }

    pub(super) fn restore_missing_downloads(&mut self) {
        self.workshop.restore_requested = true;
        self.connect_workshop();
    }

    pub(super) fn dispatch_workshop_queue(&mut self) {
        if !self.workshop.connected || self.scanning {
            return;
        }
        let mut ids: std::collections::BTreeSet<_> = std::mem::take(&mut self.workshop.resume_ids)
            .into_iter()
            .collect();
        if self.workshop.restore_requested {
            self.workshop.restore_requested = false;
            ids.extend(
                self.scan
                    .downloads
                    .iter()
                    .flat_map(|d| &d.missing)
                    .filter_map(|id| id.parse::<u64>().ok()),
            );
        }
        if ids.is_empty() {
            return;
        }
        for id in ids {
            if self.workshop.jobs.get(&id).is_some_and(Download::active) {
                continue;
            }
            if let Some(session) = &self.workshop.session {
                if let Err(error) = session.send(Command::Download { id }) {
                    self.workshop.message = error.to_string();
                    self.workshop.error = true;
                    break;
                }
                self.workshop.jobs.insert(
                    id,
                    Download {
                        id,
                        phase: Phase::Waiting,
                        downloaded: 0,
                        total: 0,
                        folder: None,
                        error: None,
                    },
                );
            }
        }
        self.save_workshop_queue();
    }
    pub(super) fn connect_workshop(&mut self) {
        if self.workshop.session.is_some() {
            return;
        }
        self.workshop.connecting = true;
        self.workshop.error = false;
        self.workshop.message = "Connecting to your Steam session…".into();
        self.workshop.session = Some(Session::spawn(&self.paths));
    }

    fn workshop_search(&mut self) {
        if !self.workshop.connected || self.workshop.searching {
            return;
        }
        self.workshop.request += 1;
        let command = Command::Search {
            request: self.workshop.request,
            search: self.workshop.search.clone(),
        };
        if let Some(session) = &self.workshop.session {
            match session.send(command) {
                Ok(()) => {
                    self.workshop.searching = true;
                    self.workshop.page = None;
                    self.workshop.message = "Searching Steam Workshop…".into();
                    self.workshop.error = false;
                }
                Err(error) => {
                    self.workshop.message = error.to_string();
                    self.workshop.error = true;
                }
            }
        }
    }

    fn workshop_download(&mut self, item: &Item) {
        if let Some(session) = &self.workshop.session {
            match session.send(Command::Download { id: item.id }) {
                Ok(()) => {
                    self.workshop.titles.insert(item.id, item.title.clone());
                    self.workshop.jobs.insert(
                        item.id,
                        Download {
                            id: item.id,
                            phase: Phase::Checking,
                            downloaded: 0,
                            total: 0,
                            folder: None,
                            error: None,
                        },
                    );
                    self.save_workshop_queue();
                }
                Err(error) => {
                    self.workshop.message = error.to_string();
                    self.workshop.error = true;
                }
            }
        }
    }

    pub(super) fn receive_workshop(&mut self) {
        let mut events = Vec::new();
        if let Some(session) = &self.workshop.session {
            loop {
                match session.try_recv() {
                    Ok(event) => {
                        let disconnected = matches!(event, SteamEvent::Disconnected { .. });
                        events.push(event);
                        if disconnected {
                            break;
                        }
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        events.push(SteamEvent::Disconnected {
                            message: "Steam connection closed. Reconnect to continue.".into(),
                        });
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                }
            }
        }
        for event in events {
            match event {
                SteamEvent::Connected => {
                    self.workshop.connected = true;
                    self.workshop.connecting = false;
                    if self.collection.syncing {
                        self.request_subscription_page();
                    }
                    if self.tab == Tab::Workshop {
                        self.workshop_search();
                    }
                }
                SteamEvent::Results { request, page } => {
                    if Some(request) == self.collection.sync_request {
                        self.receive_subscription_page(page);
                        continue;
                    }
                    if request != self.workshop.request {
                        continue;
                    }
                    self.workshop.searching = false;
                    self.workshop.error = false;
                    self.workshop.message =
                        format!("{} wallpapers · Page {}", page.total, page.page);
                    if !page
                        .items
                        .iter()
                        .any(|item| Some(item.id) == self.workshop.selected)
                    {
                        self.workshop.selected = page.items.first().map(|item| item.id);
                    }
                    self.workshop.page = Some(page);
                }
                SteamEvent::Download { download } => {
                    let complete = download.phase == Phase::Complete;
                    if complete {
                        self.message =
                            "Workshop download complete. Updating your installed wallpapers…"
                                .into();
                        self.workshop.needs_scan = true;
                        if let Some(item) = self
                            .workshop
                            .page
                            .as_mut()
                            .and_then(|p| p.items.iter_mut().find(|i| i.id == download.id))
                        {
                            item.installed = true;
                            item.subscribed = true;
                        }
                        // Steam may choose a library not previously discovered
                        // by Wallweave. Add its content root, never move files.
                        if let Some(root) = download.folder.as_ref().and_then(|p| p.parent())
                            && !self
                                .scan
                                .wallpapers
                                .iter()
                                .any(|w| w.id == download.id.to_string())
                            && !self.config.libraries.iter().any(|p| p == root)
                            && !self
                                .scan
                                .roots
                                .iter()
                                .any(|r| r.available && root.starts_with(&r.path))
                        {
                            self.config.libraries.push(root.to_path_buf());
                            if let Err(error) = self.paths.save_preferences(&self.config) {
                                self.message = format!(
                                    "Download finished; could not save its library: {error:#}"
                                );
                                self.error = true;
                            }
                        }
                    }
                    let finished = !download.active();
                    self.workshop.jobs.insert(download.id, download);
                    if finished {
                        self.save_workshop_queue();
                    }
                }
                SteamEvent::ItemTitle { id, title } => {
                    self.workshop.titles.insert(id, title);
                }
                SteamEvent::Error { request, message } => {
                    if request.is_some() && request == self.collection.sync_request {
                        self.subscription_sync_failed(message);
                        continue;
                    }
                    if request.is_some_and(|id| id != self.workshop.request) {
                        continue;
                    }
                    self.workshop.searching = false;
                    self.workshop.error = true;
                    self.workshop.message = message;
                }
                SteamEvent::Disconnected { message } => {
                    if self.collection.syncing {
                        self.subscription_sync_failed(message.clone());
                    }
                    self.workshop.session = None;
                    self.workshop.connected = false;
                    self.workshop.connecting = false;
                    self.workshop.searching = false;
                    self.workshop.error = true;
                    self.workshop.message = message.clone();
                    for job in self.workshop.jobs.values_mut().filter(|j| j.active()) {
                        job.phase = Phase::Failed;
                        job.error = Some("Steam connection interrupted. Steam may still be downloading; reconnect and retry to check.".into());
                    }
                }
            }
        }
        if self.tab != Tab::Workshop
            && self.workshop.connected
            && !self.workshop.jobs.values().any(Download::active)
            && !self.workshop.restore_requested
            && self.workshop.resume_ids.is_empty()
            && !self.collection.syncing
        {
            self.workshop.session = None;
            self.workshop.connected = false;
            self.workshop.searching = false;
        }
    }

    pub(super) fn workshop_filters(&mut self, ui: &mut egui::Ui) {
        ui.heading("Discover");
        ui.add_space(10.0);
        let mut changed = false;
        ui.add_enabled_ui(self.workshop.connected && !self.workshop.searching, |ui| {
            ui.label(RichText::new("Sort by").strong());
            egui::ComboBox::from_id_salt("workshop_order").width(145.0)
                .selected_text(self.workshop.search.order.label())
                .show_ui(ui, |ui| {
                    for &(order, label) in Order::OPTIONS {
                        changed |= ui.selectable_value(&mut self.workshop.search.order, order, label).changed();
                    }
                });
            ui.add_space(14.0);
            changed |= ui.checkbox(&mut self.workshop.search.show_nsfw, "Show NSFW")
                .on_hover_text("Include Mature and Questionable wallpapers. Steam's account content preferences still apply.").changed();
            ui.add_space(18.0);
            ui.label(RichText::new("Wallpaper type").strong());
            for &kind in workshop::KINDS {
                changed |= ui
                    .selectable_value(
                        &mut self.workshop.search.kind,
                        kind.to_owned(),
                        if kind.is_empty() { "All types" } else { kind },
                    )
                    .changed();
            }
            ui.add_space(18.0);
            ui.label(RichText::new("Resolution").strong());
            egui::ComboBox::from_id_salt("workshop_resolution")
                .width(145.0)
                .selected_text(if self.workshop.search.resolution.is_empty() {
                    "All resolutions"
                } else {
                    &self.workshop.search.resolution
                })
                .show_ui(ui, |ui| {
                    for &resolution in workshop::RESOLUTIONS {
                        changed |= ui
                            .selectable_value(
                                &mut self.workshop.search.resolution,
                                resolution.to_owned(),
                                if resolution.is_empty() {
                                    "All resolutions"
                                } else {
                                    resolution
                                },
                            )
                            .changed();
                    }
                });
            ui.add_space(18.0);
            ui.label(RichText::new("Category").strong());
            egui::ComboBox::from_id_salt("workshop_category")
                .width(145.0)
                .selected_text(if self.workshop.search.category.is_empty() {
                    "All categories"
                } else {
                    &self.workshop.search.category
                })
                .show_ui(ui, |ui| {
                    for &category in workshop::CATEGORIES {
                        changed |= ui
                            .selectable_value(
                                &mut self.workshop.search.category,
                                category.to_owned(),
                                if category.is_empty() {
                                    "All categories"
                                } else {
                                    category
                                },
                            )
                            .changed();
                    }
                });
            ui.add_space(18.0);
            if ui.button("Reset filters").clicked() {
                self.workshop.search = Search::default();
                changed = true;
            }
        });
        if changed {
            let prefs = Preferences {
                order: self.workshop.search.order,
                show_nsfw: self.workshop.search.show_nsfw,
            };
            if let Err(error) = self.write_workshop_state("workshop-preferences.json", &prefs) {
                self.workshop.message = error.to_string();
                self.workshop.error = true;
            }
            self.workshop.search.page = 1;
            self.workshop_search();
        }
        ui.add_space(18.0);
        ui.separator();
        ui.label(
            RichText::new("Downloads use your current Steam account. Steam must stay running.")
                .small()
                .color(MUTED),
        );
    }

    pub(super) fn workshop_browser(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.heading("Steam Workshop");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.workshop.connected {
                    ui.colored_label(ACCENT, "Steam connected");
                } else if self.workshop.connecting {
                    ui.spinner();
                } else if ui.button("Connect to Steam").clicked() {
                    self.connect_workshop();
                }
            });
        });
        ui.add_space(8.0);
        ui.add_enabled_ui(self.workshop.connected && !self.workshop.searching, |ui| {
            ui.horizontal(|ui| {
                ui.label("Search");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.workshop.search.text)
                        .hint_text("Words, Workshop ID or Steam link")
                        .char_limit(512)
                        .desired_width((ui.available_width() - 96.0).max(100.0)),
                );
                if ui.button("Search").clicked()
                    || response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
                {
                    self.workshop.search.page = 1;
                    self.workshop_search();
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            if self.workshop.searching {
                ui.spinner();
            }
            ui.colored_label(
                if self.workshop.error { DANGER } else { MUTED },
                &self.workshop.message,
            );
        });
        if !self.workshop.connected && !self.workshop.connecting {
            ui.add_space(20.0);
            ui.label("Start Steam and sign in with the account that owns Wallpaper Engine, then connect here.");
            ui.colored_label(MUTED, "Your existing Steam session is used. No password or API key is needed in Wallweave.");
        }
        let Some(page) = self.workshop.page.clone() else {
            return;
        };
        ui.add_enabled_ui(!self.workshop.searching && self.workshop.connected, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(page.page > 1, egui::Button::new("Previous"))
                    .clicked()
                {
                    self.workshop.search.page = page.page - 1;
                    self.workshop_search();
                }
                ui.label(format!("Page {}", page.page));
                if ui
                    .add_enabled(page.has_next, egui::Button::new("Next"))
                    .clicked()
                {
                    self.workshop.search.page = page.page + 1;
                    self.workshop_search();
                }
            });
        });
        ui.add_space(6.0);
        if page.items.is_empty() {
            ui.add_space(24.0);
            ui.heading("No matching wallpapers");
            ui.colored_label(MUTED, "Try different words or reset the filters. Item links must belong to Wallpaper Engine.");
            return;
        }
        let gap = 10.0;
        let width = (ui.available_width() - 10.0).max(100.0);
        let columns = ((width + gap) / (182.0 + gap)).floor().max(1.0) as usize;
        let tile_width = (width - gap * (columns - 1) as f32) / columns as f32;
        let tile_height = tile_width * 0.625 + 72.0;
        egui::ScrollArea::vertical()
            .id_salt(("workshop_grid", self.workshop.request))
            .auto_shrink([false, false])
            .show_rows(
                ui,
                tile_height + gap,
                page.items.len().div_ceil(columns),
                |ui, rows| {
                    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
                    for row in rows {
                        ui.horizontal(|ui| {
                            for item in &page.items
                                [row * columns..((row + 1) * columns).min(page.items.len())]
                            {
                                self.workshop_tile(
                                    ui,
                                    item,
                                    egui::vec2(tile_width, tile_height),
                                    ctx,
                                );
                            }
                        });
                    }
                },
            );
    }

    fn workshop_art(
        &mut self,
        ui: &mut egui::Ui,
        item: &Item,
        rect: egui::Rect,
        ctx: &egui::Context,
    ) {
        ui.painter()
            .rect_filled(rect, 3.0, Color32::from_rgb(24, 27, 32));
        let path = workshop::preview_path(&self.paths.state, item);
        if let Some(path) = path {
            self.request_thumbnail(&path, item.preview_url.clone(), ctx);
            if let Some(Some(texture)) = self.textures.get(&path) {
                let image_size = texture.size_vec2();
                let scale = (rect.width() / image_size.x).min(rect.height() / image_size.y);
                let target = egui::Rect::from_center_size(rect.center(), image_size * scale);
                ui.painter().image(
                    texture.id(),
                    target,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                return;
            }
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            item.kind(),
            egui::FontId::proportional(15.0),
            MUTED,
        );
    }

    fn workshop_tile(
        &mut self,
        ui: &mut egui::Ui,
        item: &Item,
        size: egui::Vec2,
        ctx: &egui::Context,
    ) {
        let response = ui
            .push_id(item.id, |ui| {
                ui.add_sized(
                    size,
                    egui::Button::new("")
                        .selected(self.workshop.selected == Some(item.id))
                        .fill(SURFACE),
                )
            })
            .inner;
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                true,
                self.workshop.selected == Some(item.id),
                &item.title,
            )
        });
        if response.clicked() {
            self.workshop.selected = Some(item.id);
        }
        let rect = response.rect.shrink(6.0);
        let image_rect =
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), size.x * 0.625 - 8.0));
        self.workshop_art(ui, item, image_rect, ctx);
        let mut title_job = egui::text::LayoutJob::simple(
            item.title.clone(),
            egui::FontId::proportional(14.0),
            TEXT,
            rect.width(),
        );
        title_job.wrap.max_rows = 2;
        let title = ui.fonts(|fonts| fonts.layout_job(title_job));
        let text_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left(), image_rect.bottom() + 6.0),
            egui::vec2(rect.width(), 34.0),
        );
        ui.painter()
            .with_clip_rect(text_rect)
            .galley(text_rect.min, title, TEXT);
        let installed = self
            .scan
            .wallpapers
            .iter()
            .any(|w| w.id == item.id.to_string());
        let label = if let Some(job) = self.workshop.jobs.get(&item.id) {
            phase_label(&job.phase)
        } else if installed || item.installed {
            "Installed"
        } else {
            item.kind()
        };
        ui.painter().with_clip_rect(response.rect).text(
            egui::pos2(rect.left(), rect.bottom() - 2.0),
            egui::Align2::LEFT_BOTTOM,
            label,
            egui::FontId::proportional(12.0),
            if installed { ACCENT } else { MUTED },
        );
        response.on_hover_text(format!("{}\n{} · {}", item.title, item.kind(), item.id));
    }

    pub(super) fn workshop_detail(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let item = self
            .workshop
            .page
            .as_ref()
            .and_then(|p| {
                p.items
                    .iter()
                    .find(|i| Some(i.id) == self.workshop.selected)
            })
            .cloned();
        if let Some(item) = item {
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), ui.available_width() * 0.625),
                egui::Sense::hover(),
            );
            self.workshop_art(ui, &item, rect, ctx);
            ui.add_space(10.0);
            ui.heading(&item.title);
            ui.colored_label(MUTED, format!("{} · {}", item.kind(), item.id));
            if item.nsfw {
                ui.colored_label(DANGER, "NSFW");
            }
            ui.label(format!(
                "{} · {} positive votes",
                bytes(item.bytes),
                item.votes
            ));
            ui.add_space(10.0);
            let local = self
                .scan
                .wallpapers
                .iter()
                .find(|w| w.id == item.id.to_string())
                .cloned();
            let job = self.workshop.jobs.get(&item.id).cloned();
            if let Some(local) = local {
                if ui
                    .add_sized(
                        [ui.available_width(), 40.0],
                        egui::Button::new(RichText::new("Open in Installed").color(BG))
                            .fill(ACCENT),
                    )
                    .clicked()
                {
                    self.select(&local, ctx);
                    self.tab = Tab::Library;
                }
            } else {
                let active = job.as_ref().is_some_and(Download::active);
                let failed = job.as_ref().is_some_and(|j| j.phase == Phase::Failed);
                let unsupported = item.kind() == "Application";
                let label = if active {
                    "Download in progress…"
                } else if failed {
                    "Retry download"
                } else if item.installed {
                    "Locate installed wallpaper"
                } else {
                    "Download & subscribe"
                };
                if ui
                    .add_enabled(
                        self.workshop.connected && !active && !unsupported,
                        egui::Button::new(RichText::new(label).color(BG))
                            .min_size(egui::vec2(ui.available_width(), 40.0))
                            .fill(ACCENT),
                    )
                    .clicked()
                {
                    self.workshop_download(&item);
                }
                if unsupported {
                    ui.colored_label(
                        DANGER,
                        "Windows application wallpapers cannot run in Wallweave.",
                    );
                } else {
                    ui.label(RichText::new("Steam stores the files in its library and keeps the wallpaper updated.").small().color(MUTED));
                }
            }
            ui.hyperlink_to(
                "View Workshop page",
                format!(
                    "https://steamcommunity.com/sharedfiles/filedetails/?id={}",
                    item.id
                ),
            );
            if !item.tags.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new(item.tags.join(" · ")).small().color(MUTED));
            }
        } else {
            ui.heading("Choose a wallpaper");
            ui.colored_label(MUTED, "Select a preview to see details and download it.");
        }
        if !self.workshop.jobs.is_empty() {
            ui.add_space(18.0);
            ui.separator();
            let active = self.workshop.jobs.values().filter(|j| j.active()).count();
            ui.heading(format!("Downloads · {active} active"));
            ui.colored_label(MUTED, "Steam manages the download queue.");
            let jobs: Vec<_> = self.workshop.jobs.values().cloned().collect();
            for job in jobs {
                ui.push_id(("download", job.id), |ui| {
                    ui.add_space(8.0);
                    let title = self
                        .workshop
                        .titles
                        .get(&job.id)
                        .cloned()
                        .unwrap_or_else(|| job.id.to_string());
                    ui.add(egui::Label::new(RichText::new(title).strong()).wrap());
                    ui.colored_label(
                        if job.phase == Phase::Failed {
                            DANGER
                        } else {
                            MUTED
                        },
                        phase_label(&job.phase),
                    );
                    if let Some(fraction) = job.fraction() {
                        ui.add(egui::ProgressBar::new(fraction).text(format!(
                            "{} / {}",
                            bytes(job.downloaded),
                            bytes(job.total)
                        )));
                    } else if job.active() {
                        ui.spinner();
                    }
                    if let Some(error) = &job.error {
                        ui.colored_label(DANGER, error);
                    }
                    if job.phase == Phase::Complete {
                        if ui.button("Show in Installed").clicked() {
                            if let Some(local) = self
                                .scan
                                .wallpapers
                                .iter()
                                .find(|w| w.id == job.id.to_string())
                                .cloned()
                            {
                                self.select(&local, ctx);
                                self.tab = Tab::Library;
                            } else {
                                self.refresh(ctx);
                            }
                        }
                    } else if job.phase == Phase::Failed
                        && self.workshop.connected
                        && ui.button("Retry").clicked()
                        && let Some(session) = &self.workshop.session
                    {
                        match session.send(Command::Download { id: job.id }) {
                            Ok(()) => {
                                if let Some(job) = self.workshop.jobs.get_mut(&job.id) {
                                    job.phase = Phase::Checking;
                                    job.error = None;
                                }
                            }
                            Err(error) => {
                                self.workshop.message = error.to_string();
                                self.workshop.error = true;
                            }
                        }
                    }
                });
            }
        }
    }
}
