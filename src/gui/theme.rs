pub(super) use super::icons::Icon;
use super::*;

pub(super) const ACTIVE: Color32 = Color32::from_rgb(38, 60, 87);
pub(super) const SUCCESS: Color32 = Color32::from_rgb(142, 218, 182);

pub(super) fn heading(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).font(egui::FontId::new(
        size,
        egui::FontFamily::Name("heading".into()),
    ))
}

pub(super) fn icon(ui: &mut egui::Ui, glyph: Icon, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    paint_icon(ui, rect, glyph, color);
}

pub(super) fn paint_icon(ui: &egui::Ui, rect: egui::Rect, glyph: Icon, color: Color32) {
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph.glyph(),
        egui::FontId::new(rect.height(), egui::FontFamily::Name("icons".into())),
        color,
    );
}

pub(super) fn button_text(icon: Icon, text: &str, color: Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &icon.glyph().to_string(),
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(17.0, egui::FontFamily::Name("icons".into())),
            color,
            ..Default::default()
        },
    );
    job.append(
        text,
        9.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(14.0),
            color,
            ..Default::default()
        },
    );
    job
}

pub(super) fn button(ui: &mut egui::Ui, icon: Icon, text: &str, primary: bool) -> egui::Response {
    ui.add(
        egui::Button::new(button_text(icon, text, if primary { BG } else { TEXT }))
            .fill(if primary { ACCENT } else { SURFACE })
            .min_size(egui::vec2(0.0, 38.0)),
    )
}

pub(super) fn wide_button(
    ui: &mut egui::Ui,
    icon: Icon,
    text: &str,
    primary: bool,
) -> egui::Response {
    ui.add_sized(
        [ui.available_width(), if primary { 44.0 } else { 38.0 }],
        egui::Button::new(button_text(icon, text, if primary { BG } else { TEXT }))
            .fill(if primary { ACCENT } else { SURFACE }),
    )
}

pub(super) fn nav(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    count: Option<usize>,
    selected: bool,
) -> egui::Response {
    let response = ui.add_sized(
        [ui.available_width(), 40.0],
        egui::Button::new("")
            .fill(if selected {
                ACTIVE
            } else {
                Color32::TRANSPARENT
            })
            .stroke(egui::Stroke::NONE),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
    });
    let r = response.rect;
    let color = if selected { TEXT } else { MUTED };
    paint_icon(
        ui,
        egui::Rect::from_center_size(
            egui::pos2(r.left() + 18.0, r.center().y),
            egui::vec2(17.0, 17.0),
        ),
        icon,
        if selected { ACCENT } else { MUTED },
    );
    let count_space = count.map_or(0.0, |n| n.to_string().len() as f32 * 7.0 + 12.0);
    let mut text = egui::text::LayoutJob::simple(
        label.into(),
        egui::FontId::proportional(13.0),
        color,
        (r.width() - 46.0 - count_space).max(20.0),
    );
    text.wrap.max_rows = 1;
    let galley = ui.fonts(|f| f.layout_job(text));
    ui.painter().galley(
        egui::pos2(r.left() + 36.0, r.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    if let Some(count) = count {
        ui.painter().text(
            egui::pos2(r.right() - 10.0, r.center().y),
            egui::Align2::RIGHT_CENTER,
            count.to_string(),
            egui::FontId::proportional(12.0),
            if selected { ACCENT } else { MUTED },
        );
    }
    response.on_hover_text(label)
}

pub(super) fn tab(ui: &mut egui::Ui, icon: Icon, label: &str, selected: bool) -> egui::Response {
    let response = ui.add(
        egui::Button::new(button_text(
            icon,
            label,
            if selected { TEXT } else { MUTED },
        ))
        .fill(Color32::TRANSPARENT)
        .stroke(egui::Stroke::NONE)
        .min_size(egui::vec2(0.0, 54.0)),
    );
    if selected {
        ui.painter().hline(
            response.rect.x_range(),
            response.rect.bottom() - 1.0,
            egui::Stroke::new(3.0_f32, ACCENT),
        );
    }
    response
}

pub(super) fn section() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(egui::Stroke::new(1.0_f32, BORDER))
        .corner_radius(8.0)
        .inner_margin(22.0)
}

pub(super) fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(7, 4))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).color(color));
        });
}

pub(super) fn status(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 12.0), egui::Sense::hover());
        ui.painter().circle_filled(r.center(), 3.0, color);
        ui.label(RichText::new(text).size(12.0).color(color));
    });
}

pub(super) fn kind(kind: &str) -> &str {
    match kind {
        "scene" | "Scene" => "Szene",
        "video" | "Video" => "Video",
        "web" | "Web" => "Web",
        "application" | "Application" => "Anwendung",
        _ => kind,
    }
}

pub(super) fn playback(state: &str) -> &str {
    match state {
        "Playing" => "Wird wiedergegeben",
        "Paused" => "Pausiert",
        "Paused automatically" => "Automatisch pausiert",
        "Saved · not running" => "Gespeichert · nicht aktiv",
        _ => state,
    }
}

pub(super) fn image(ui: &egui::Ui, texture: &egui::TextureHandle, rect: egui::Rect, crop: bool) {
    let size = texture.size_vec2();
    let mut uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
    let mut target = rect;
    if crop {
        let ratio = (rect.width() / rect.height()) / (size.x / size.y);
        if ratio < 1.0 {
            uv.min.x = (1.0 - ratio) / 2.0;
            uv.max.x = 1.0 - uv.min.x;
        } else {
            uv.min.y = (1.0 - 1.0 / ratio) / 2.0;
            uv.max.y = 1.0 - uv.min.y;
        }
    } else {
        target = egui::Rect::from_center_size(
            rect.center(),
            size * (rect.width() / size.x).min(rect.height() / size.y),
        );
    }
    egui::Image::from_texture((texture.id(), size))
        .uv(uv)
        .corner_radius(6.0)
        .paint_at(ui, target);
}

pub(super) fn slider(ui: &mut egui::Ui, slider: egui::Slider<'_>) -> egui::Response {
    ui.scope(|ui| {
        ui.visuals_mut().selection.bg_fill = ACCENT;
        ui.spacing_mut().interact_size.y = 22.0;
        ui.add(slider)
    })
    .inner
}
