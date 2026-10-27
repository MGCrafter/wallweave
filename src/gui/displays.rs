use super::playback::DisplayWallpaper;
use super::{ACCENT, BG, BORDER, MUTED, PANEL, SURFACE, TEXT, theme};
use crate::desktop::Monitor;
use eframe::egui::{self, RichText};
use std::collections::BTreeMap;
use std::time::Instant;

const OPEN_SECONDS: f32 = 0.22;

pub(super) struct DisplayPicker {
    pub previews: BTreeMap<String, egui::TextureHandle>,
    opened_at: Option<Instant>,
    startup_pending: bool,
    focus_pending: bool,
}

impl Default for DisplayPicker {
    fn default() -> Self {
        Self {
            previews: BTreeMap::new(),
            opened_at: None,
            startup_pending: true,
            focus_pending: false,
        }
    }
}

impl DisplayPicker {
    pub fn dismiss(&mut self) {
        self.startup_pending = false;
        self.opened_at = None;
        self.focus_pending = false;
    }

    pub fn open(&mut self) {
        self.opened_at = Some(Instant::now());
        self.focus_pending = true;
    }

    pub fn monitors_ready(&mut self, monitors: &[Monitor]) {
        if self.startup_pending && !monitors.is_empty() {
            self.startup_pending = false;
            self.open();
        }
    }

    pub fn settled(&self, ctx: &egui::Context) -> bool {
        ctx.style().animation_time <= 0.0
            || self
                .opened_at
                .is_none_or(|at| at.elapsed().as_secs_f32() >= OPEN_SECONDS)
    }

    /// Returns true only when the user chooses a target; playback is separate.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        monitors: &[Monitor],
        output: &mut String,
        assignments: &BTreeMap<String, DisplayWallpaper>,
    ) -> bool {
        let Some(opened_at) = self.opened_at else {
            return false;
        };
        let progress = if ctx.style().animation_time <= 0.0 {
            1.0
        } else {
            (opened_at.elapsed().as_secs_f32() / OPEN_SECONDS).clamp(0.0, 1.0)
        };
        let opacity = 1.0 - (1.0 - progress).powi(3);
        if progress < 1.0 {
            ctx.request_repaint();
        }
        let width = (ctx.screen_rect().width() - 64.0).clamp(220.0, 756.0);
        let mut frame = egui::Frame::popup(&ctx.style())
            .fill(PANEL.gamma_multiply(opacity))
            .stroke(egui::Stroke::new(1.0_f32, BORDER.gamma_multiply(opacity)))
            .inner_margin(28.0)
            .corner_radius(12.0);
        frame.shadow.color = frame.shadow.color.gamma_multiply(opacity);
        let area = egui::Modal::default_area(egui::Id::new("display_picker"))
            .anchor(
                egui::Align2::CENTER_CENTER,
                egui::vec2(0.0, -16.0 * (1.0 - opacity)),
            )
            .fade_in(false);
        let mut chosen = false;
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("display_picker"))
            .area(area)
            .frame(frame)
            .backdrop_color(egui::Color32::from_black_alpha((170.0 * opacity) as u8))
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.set_opacity(opacity);
                ui.horizontal(|ui| {
                    ui.label(theme::heading("Deine Bildschirme", 26.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = ui.button("Schließen").clicked();
                    });
                });
                ui.colored_label(
                    MUTED,
                    "Wähle aus, welchen Bildschirm du gestalten möchtest.",
                );
                ui.add_space(12.0);
                egui::ScrollArea::vertical()
                    .id_salt("display_picker_content")
                    .max_height((ctx.screen_rect().height() - 190.0).max(120.0))
                    .show(ui, |ui| {
                        if monitors.is_empty() {
                            ui.label(
                                "Bildschirme konnten auf diesem Desktop nicht erkannt werden.",
                            );
                            ui.label("Bildschirmanschluss");
                            ui.text_edit_singleline(output);
                            chosen = ui
                                .add_enabled(
                                    !output.trim().is_empty(),
                                    egui::Button::new("Bildschirm verwenden"),
                                )
                                .clicked();
                            return;
                        }
                        let height = (ctx.screen_rect().height() * 0.40).clamp(150.0, 250.0);
                        let (canvas, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), height),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(canvas, 8.0, BG);
                        let rects = display_rects(monitors, canvas.shrink(12.0));
                        let compact = rects.iter().any(|r| r.width() < 108.0 || r.height() < 66.0);
                        let overlapping = rects.iter().enumerate().any(|(i, rect)| {
                            rects
                                .iter()
                                .skip(i + 1)
                                .any(|other| rect.intersects(*other))
                        });
                        for (index, (monitor, rect)) in monitors.iter().zip(&rects).enumerate() {
                            let selected = *output == monitor.name;
                            let response = ui
                                .put(
                                    *rect,
                                    egui::Button::new("")
                                        .selected(selected)
                                        .fill(if selected {
                                            egui::Color32::from_rgb(40, 68, 97)
                                        } else {
                                            SURFACE
                                        })
                                        .stroke(egui::Stroke::new(
                                            if selected { 2.0_f32 } else { 1.0_f32 },
                                            if selected { ACCENT } else { MUTED },
                                        )),
                                )
                                .on_hover_text(format!(
                                    "{}\n{}\n{} × {}\n{}",
                                    monitor.name,
                                    monitor.description,
                                    monitor.width,
                                    monitor.height,
                                    assignments
                                        .get(&monitor.name)
                                        .map(|w| format!("{}: {}", w.state, w.title))
                                        .unwrap_or_else(|| "Kein Wallpaper zugewiesen".into()),
                                ));
                            response.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::Button,
                                    true,
                                    selected,
                                    format!(
                                        "Display {}: {}, {} by {}",
                                        index + 1,
                                        monitor.name,
                                        monitor.width,
                                        monitor.height
                                    ),
                                )
                            });
                            if self.focus_pending && selected {
                                response.request_focus();
                            }
                            if response.has_focus() {
                                ui.painter().rect_stroke(
                                    rect.expand(3.0),
                                    5.0,
                                    egui::Stroke::new(2.0_f32, ACCENT),
                                    egui::StrokeKind::Outside,
                                );
                            }
                            if let Some(texture) = self.previews.get(&monitor.name) {
                                theme::image(ui, texture, rect.shrink(3.0), true);
                                ui.painter().rect_filled(
                                    rect.shrink(3.0),
                                    6.0,
                                    egui::Color32::from_black_alpha(100),
                                );
                            }
                            let center = rect.center();
                            let full_label = rect.width() >= 108.0 && rect.height() >= 66.0;
                            let short = rect.height() < 84.0;
                            ui.painter().text(
                                center - egui::vec2(0.0, if full_label { 20.0 } else { 0.0 }),
                                egui::Align2::CENTER_CENTER,
                                (index + 1).to_string(),
                                egui::FontId::proportional(if full_label && !short {
                                    26.0
                                } else {
                                    20.0
                                }),
                                if selected { ACCENT } else { TEXT },
                            );
                            if full_label {
                                ui.painter().text(
                                    center + egui::vec2(0.0, if short { 4.0 } else { 9.0 }),
                                    egui::Align2::CENTER_CENTER,
                                    &monitor.name,
                                    egui::FontId::proportional(14.0),
                                    TEXT,
                                );
                                ui.painter().text(
                                    center + egui::vec2(0.0, if short { 24.0 } else { 29.0 }),
                                    egui::Align2::CENTER_CENTER,
                                    format!("{} × {}", monitor.width, monitor.height),
                                    egui::FontId::proportional(12.0),
                                    MUTED,
                                );
                            }
                            if response.clicked() {
                                *output = monitor.name.clone();
                                chosen = true;
                            }
                        }
                        self.focus_pending = false;
                        ui.add_space(10.0);
                        ui.label(RichText::new("Auf deinen Bildschirmen").strong());
                        for monitor in monitors {
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(*output == monitor.name, &monitor.name)
                                    .clicked()
                                {
                                    *output = monitor.name.clone();
                                    chosen = true;
                                }
                                ui.vertical(|ui| {
                                    if let Some(current) = assignments.get(&monitor.name) {
                                        ui.add(egui::Label::new(&current.title).truncate())
                                            .on_hover_text(&current.title);
                                        ui.label(
                                            RichText::new(theme::playback(current.state))
                                                .small()
                                                .color(MUTED),
                                        );
                                    } else {
                                        ui.label(
                                            RichText::new("Kein Wallpaper zugewiesen").color(MUTED),
                                        );
                                    }
                                });
                            });
                        }
                        if compact || overlapping {
                            ui.add_space(8.0);
                            ui.horizontal_wrapped(|ui| {
                                for (index, monitor) in monitors.iter().enumerate() {
                                    if ui
                                        .selectable_label(
                                            *output == monitor.name,
                                            format!("{}  {}", index + 1, monitor.name),
                                        )
                                        .clicked()
                                    {
                                        *output = monitor.name.clone();
                                        chosen = true;
                                    }
                                }
                            });
                        }
                        if monitors.iter().any(|m| m.x.is_none() || m.y.is_none()) {
                            ui.colored_label(
                                MUTED,
                                "Bildschirmpositionen sind nicht verfügbar; Anzeige nebeneinander.",
                            );
                        }
                    });
                ui.add_space(12.0);
                ui.label(
                    RichText::new("Das Wallpaper wendest du danach in der Bibliothek an.")
                        .small()
                        .color(MUTED),
                );
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Ausgewählt: {output}"))
                            .strong()
                            .color(ACCENT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                !output.is_empty(),
                                egui::Button::new(RichText::new("Bildschirm auswählen").color(BG))
                                    .fill(ACCENT),
                            )
                            .clicked()
                        {
                            chosen = true;
                        }
                    });
                });
            });
        if chosen || close || response.should_close() {
            self.opened_at = None;
        }
        chosen
    }
}

/// Scale logical compositor coordinates uniformly, including negative origins.
fn display_rects(monitors: &[Monitor], canvas: egui::Rect) -> Vec<egui::Rect> {
    let known_positions = monitors.iter().all(|m| m.x.is_some() && m.y.is_some());
    let mut cursor = 0.0;
    let raw: Vec<_> = monitors
        .iter()
        .map(|m| {
            let (width, height) = m.logical_size();
            let origin = if known_positions {
                egui::pos2(
                    m.x.unwrap_or_default() as f32,
                    m.y.unwrap_or_default() as f32,
                )
            } else {
                let x = cursor;
                cursor += width + 80.0;
                egui::pos2(x, 0.0)
            };
            egui::Rect::from_min_size(origin, egui::vec2(width, height))
        })
        .collect();
    let bounds = raw.iter().copied().reduce(|a, b| a.union(b));
    let Some(bounds) = bounds else {
        return vec![];
    };
    let scale = (canvas.width() / bounds.width()).min(canvas.height() / bounds.height());
    let origin = canvas.center() - bounds.size() * scale * 0.5;
    raw.into_iter()
        .map(|rect| {
            let scaled = egui::Rect::from_min_size(
                origin + (rect.min - bounds.min) * scale,
                rect.size() * scale,
            );
            // Leave a small, bounded gap even when physical monitor edges touch.
            scaled.shrink(
                3.0_f32
                    .min(scaled.width() * 0.08)
                    .min(scaled.height() * 0.08),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn monitors() -> Vec<Monitor> {
        serde_json::from_value(serde_json::json!([
            {"name":"HDMI-A-1","width":1920,"height":1080,"scale":1.0,"x":-80,"y":-40},
            {"name":"DP-1","width":3440,"height":1440,"scale":1.0,"x":1270,"y":-1480},
            {"name":"DP-2","width":1920,"height":1080,"scale":1.0,"x":1840,"y":-40,"focused":true}
        ]))
        .expect("monitor fixture")
    }

    #[test]
    fn arrangement_fits_and_keeps_the_real_three_screen_relationships() {
        let monitors = monitors();
        for size in [egui::vec2(280.0, 150.0), egui::vec2(536.0, 226.0)] {
            let canvas = egui::Rect::from_min_size(egui::pos2(30.0, 90.0), size);
            let rects = display_rects(&monitors, canvas);
            assert_eq!(rects.len(), 3);
            assert!(
                rects
                    .iter()
                    .all(|r| canvas.contains_rect(*r) && r.is_positive())
            );
            assert!(
                rects[1].bottom() < rects[0].top(),
                "ultrawide above left display"
            );
            assert!(
                rects[1].bottom() < rects[2].top(),
                "ultrawide above right display"
            );
            assert!(
                rects[0].right() < rects[2].left(),
                "HDMI remains to the left of DP-2"
            );
            assert!((rects[0].center().y - rects[2].center().y).abs() < 0.01);
        }
    }

    fn frame(
        picker: &mut DisplayPicker,
        ctx: &egui::Context,
        monitors: &[Monitor],
        output: &mut String,
        events: Vec<egui::Event>,
    ) -> (bool, egui::FullOutput) {
        let mut chosen = false;
        let rendered = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(916.0, 521.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                chosen |= picker.show(ctx, monitors, output, &BTreeMap::new());
            },
        );
        (chosen, rendered)
    }

    #[test]
    fn picker_shows_wallpaper_titles_and_playback_state_for_each_display() {
        let ctx = egui::Context::default();
        crate::fonts::configure(&ctx);
        let monitors = monitors();
        let mut picker = DisplayPicker::default();
        picker.monitors_ready(&monitors);
        picker.opened_at = Some(Instant::now() - Duration::from_secs(1));
        let mut output = "DP-2".into();
        let assignments = BTreeMap::from([
            (
                "DP-1".into(),
                DisplayWallpaper {
                    folder: "/one".into(),
                    title: "Forest wallpaper".into(),
                    state: "Playing",
                },
            ),
            (
                "DP-2".into(),
                DisplayWallpaper {
                    folder: "/two".into(),
                    title: "Ocean wallpaper".into(),
                    state: "Paused",
                },
            ),
        ]);
        let mut rendered = None;
        for _ in 0..3 {
            rendered = Some(ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 840.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    assert!(!picker.show(ctx, &monitors, &mut output, &assignments));
                },
            ));
        }
        fn contains(shape: &egui::Shape, value: &str) -> bool {
            match shape {
                egui::Shape::Text(text) => text.galley.text() == value,
                egui::Shape::Vec(shapes) => shapes.iter().any(|shape| contains(shape, value)),
                _ => false,
            }
        }
        let rendered = rendered.expect("rendered picker");
        for text in [
            "Forest wallpaper",
            "Ocean wallpaper",
            "Wird wiedergegeben",
            "Pausiert",
            "Kein Wallpaper zugewiesen",
        ] {
            assert!(
                rendered
                    .shapes
                    .iter()
                    .any(|shape| contains(&shape.shape, text)),
                "Missing {text}"
            );
        }
        assert_eq!(
            output, "DP-2",
            "Viewing assignments does not change the selected output"
        );
    }

    #[test]
    fn clicking_a_monitor_selects_that_connector_and_refresh_does_not_reopen() {
        let ctx = egui::Context::default();
        crate::fonts::configure(&ctx);
        let monitors = monitors();
        let mut picker = DisplayPicker::default();
        let mut output = "DP-2".to_string();
        picker.monitors_ready(&monitors);
        assert!(picker.opened_at.is_some());
        picker.opened_at = Some(Instant::now() - Duration::from_secs(1));
        let mut rendered = None;
        for _ in 0..3 {
            rendered = Some(frame(&mut picker, &ctx, &monitors, &mut output, vec![]).1);
        }
        // Hit the monitor card from its painted numeric label, not a guessed UI coordinate.
        fn number_position(shape: &egui::epaint::Shape) -> Option<egui::Pos2> {
            match shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == "1" => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(number_position),
                _ => None,
            }
        }
        let pos = rendered
            .expect("frame")
            .shapes
            .iter()
            .find_map(|shape| number_position(&shape.shape))
            .expect("HDMI monitor card");
        let click = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &mut picker,
            &ctx,
            &monitors,
            &mut output,
            vec![egui::Event::PointerMoved(pos), click(true)],
        );
        assert!(
            frame(
                &mut picker,
                &ctx,
                &monitors,
                &mut output,
                vec![click(false)]
            )
            .0
        );
        assert_eq!(output, "HDMI-A-1");
        assert!(picker.opened_at.is_none());
        picker.monitors_ready(&monitors);
        assert!(
            picker.opened_at.is_none(),
            "background refresh must not reopen startup dialog"
        );

        picker.open();
        frame(
            &mut picker,
            &ctx,
            &monitors,
            &mut output,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(picker.opened_at.is_none());
        assert_eq!(output, "HDMI-A-1", "Escape keeps the existing target");
    }

    #[test]
    fn keyboard_activation_chooses_the_focused_monitor_without_motion() {
        let ctx = egui::Context::default();
        crate::fonts::configure(&ctx);
        ctx.style_mut(|style| style.animation_time = 0.0);
        let monitors = monitors();
        let mut picker = DisplayPicker::default();
        let mut output = "DP-2".into();
        picker.monitors_ready(&monitors);
        assert!(picker.settled(&ctx));
        for _ in 0..3 {
            frame(&mut picker, &ctx, &monitors, &mut output, vec![]);
        }
        let key = |pressed| egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let first = frame(&mut picker, &ctx, &monitors, &mut output, vec![key(true)]).0;
        let second = frame(&mut picker, &ctx, &monitors, &mut output, vec![key(false)]).0;
        assert!(first || second, "Enter activates the focused screen");
        assert_eq!(output, "DP-2");
        assert!(picker.opened_at.is_none());
    }
}
