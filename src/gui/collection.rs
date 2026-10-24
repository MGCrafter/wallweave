use super::*;
use crate::workshop::{Command, Order, Page, Search};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Sort {
    #[default]
    Title,
    Type,
    SubscribedNewest,
    SubscribedOldest,
}

impl Sort {
    pub const OPTIONS: &[(Self, &str)] = &[
        (Self::Title, "Title"),
        (Self::Type, "Type"),
        (Self::SubscribedNewest, "Subscribed: newest first"),
        (Self::SubscribedOldest, "Subscribed: oldest first"),
    ];

    pub fn label(self) -> &'static str {
        Self::OPTIONS
            .iter()
            .find(|(sort, _)| *sort == self)
            .map(|(_, label)| *label)
            .unwrap_or("Title")
    }

    pub fn uses_subscription_date(self) -> bool {
        matches!(self, Self::SubscribedNewest | Self::SubscribedOldest)
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Preferences {
    pub sort: Sort,
    pub favorites_only: bool,
    pub favorites: BTreeSet<String>,
    pub subscription_dates: BTreeMap<u64, u32>,
}

pub(super) struct CollectionUi {
    pub preferences: Preferences,
    pub syncing: bool,
    pub sync_request: Option<u64>,
    pub attempted: bool,
    pub message: String,
    pub error: bool,
    request: u64,
    page: u32,
    pending_dates: BTreeMap<u64, u32>,
}

impl CollectionUi {
    pub fn load(state: &Path) -> Self {
        let loaded = match std::fs::read(state.join("library-preferences.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(anyhow::Error::from),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(Preferences::default())
            }
            Err(error) => Err(error.into()),
        };
        let error = loaded
            .as_ref()
            .err()
            .map(|e| format!("Could not load library preferences: {e}"));
        Self {
            preferences: loaded.unwrap_or_default(),
            syncing: false,
            sync_request: None,
            attempted: false,
            message: error.clone().unwrap_or_default(),
            error: error.is_some(),
            // Browser queries use the lower half of the shared request space.
            request: 1 << 63,
            page: 1,
            pending_dates: BTreeMap::new(),
        }
    }

    pub fn is_favorite(&self, wallpaper: &Wallpaper) -> bool {
        self.preferences
            .favorites
            .contains(&wallpaper.favorite_key())
    }

    pub fn subscribed_at(&self, wallpaper: &Wallpaper) -> Option<u32> {
        self.preferences
            .subscription_dates
            .get(&wallpaper.workshop_id()?)
            .copied()
            .filter(|date| *date > 0)
    }
}

impl App {
    pub(super) fn save_collection_preferences(&mut self, preferences: Preferences) -> bool {
        match self.write_workshop_state("library-preferences.json", &preferences) {
            Ok(()) => {
                self.collection.preferences = preferences;
                self.collection.error = false;
                self.collection.message.clear();
                true
            }
            Err(error) => {
                self.collection.message = format!("Could not save library preferences: {error:#}");
                self.collection.error = true;
                false
            }
        }
    }

    pub(super) fn toggle_favorite(&mut self, wallpaper: &Wallpaper) {
        let mut preferences = self.collection.preferences.clone();
        let key = wallpaper.favorite_key();
        let removed = preferences.favorites.remove(&key);
        if !removed {
            preferences.favorites.insert(key);
        }
        if self.save_collection_preferences(preferences) {
            self.message = format!(
                "{} {} favorites.",
                wallpaper.title,
                if removed { "removed from" } else { "added to" },
            );
            self.error = false;
        }
    }

    pub(super) fn sync_subscription_dates(&mut self) {
        if self.collection.syncing {
            return;
        }
        self.collection.attempted = true;
        self.collection.syncing = true;
        self.collection.page = 1;
        self.collection.pending_dates.clear();
        self.collection.error = false;
        self.collection.message = "Reading subscription dates from Steam…".into();
        if self.workshop.connected {
            self.request_subscription_page();
        } else {
            self.connect_workshop();
        }
    }

    pub(super) fn request_subscription_page(&mut self) {
        self.collection.request += 1;
        let request = self.collection.request;
        self.collection.sync_request = Some(request);
        let result = self
            .workshop
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Connect to Steam to read subscription dates."))
            .and_then(|session| {
                session.send(Command::Search {
                    request,
                    search: Search {
                        order: Order::Subscriptions,
                        page: self.collection.page,
                        // Installed content is independent of the browser's filters.
                        show_nsfw: true,
                        ..Default::default()
                    },
                })
            });
        if let Err(error) = result {
            self.subscription_sync_failed(format!("{error:#}"));
        }
    }

    pub(super) fn receive_subscription_page(&mut self, page: Page) {
        self.collection
            .pending_dates
            .extend(page.items.iter().filter_map(|item| {
                item.subscribed_at
                    .filter(|date| *date > 0)
                    .map(|date| (item.id, date))
            }));
        if page.has_next {
            self.collection.page += 1;
            self.collection.message = format!(
                "Reading Steam subscription dates · page {}…",
                self.collection.page
            );
            self.request_subscription_page();
            return;
        }
        self.collection.syncing = false;
        self.collection.sync_request = None;
        let mut preferences = self.collection.preferences.clone();
        preferences.subscription_dates = std::mem::take(&mut self.collection.pending_dates);
        let count = preferences.subscription_dates.len();
        if self.save_collection_preferences(preferences) {
            self.collection.message =
                format!("{count} Steam subscription dates saved for offline use.");
        }
    }

    pub(super) fn subscription_sync_failed(&mut self, message: String) {
        self.collection.syncing = false;
        self.collection.sync_request = None;
        self.collection.pending_dates.clear();
        self.collection.message =
            format!("Subscription dates: {message} Saved dates are still available.");
        self.collection.error = true;
    }

    pub(super) fn favorite_button(
        &mut self,
        ui: &mut egui::Ui,
        wallpaper: &Wallpaper,
        rect: egui::Rect,
    ) -> egui::Response {
        let favorite = self.collection.is_favorite(wallpaper);
        let response = ui.put(rect, egui::Button::new("").fill(BG).selected(favorite));
        let label = format!(
            "{} favorite: {}",
            if favorite { "Remove" } else { "Add" },
            wallpaper.title
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, favorite, &label)
        });
        let center = response.rect.center();
        let points: Vec<_> = (0..10)
            .map(|i| {
                let angle = std::f32::consts::TAU * i as f32 / 10.0 - std::f32::consts::FRAC_PI_2;
                center + egui::vec2(angle.cos(), angle.sin()) * if i % 2 == 0 { 9.0 } else { 4.0 }
            })
            .collect();
        let color = if favorite || response.hovered() {
            ACCENT
        } else {
            TEXT
        };
        if favorite {
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(center, color);
            for point in &points {
                mesh.colored_vertex(*point, color);
            }
            for i in 0..10 {
                mesh.add_triangle(0, i + 1, (i + 1) % 10 + 1);
            }
            ui.painter().add(mesh);
        }
        ui.painter().add(egui::Shape::closed_line(
            points,
            egui::Stroke::new(1.5_f32, color),
        ));
        if response.clicked() {
            self.toggle_favorite(wallpaper);
        }
        response.on_hover_text(label)
    }
}
