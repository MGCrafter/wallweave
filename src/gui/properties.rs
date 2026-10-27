use super::{MUTED, Wallpaper};
use crate::config::Assignment;
use eframe::egui::{self, RichText};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

type Property<'a> = (&'a str, &'a Value);
struct Section<'a> {
    group: Option<Property<'a>>,
    rows: Vec<Property<'a>>,
}

#[derive(Default)]
pub(super) struct PropertiesUi {
    pub details: bool,
    search: String,
    modified_only: bool,
    drafts: HashMap<String, BTreeMap<String, String>>,
    pub file_request: Option<(String, bool)>,
}

impl PropertiesUi {
    pub fn set_file(&mut self, wallpaper: &Wallpaper, key: String, path: String) {
        self.drafts
            .entry(wallpaper.favorite_key())
            .or_default()
            .insert(key, path);
    }

    pub fn remember(&mut self, wallpaper: &Wallpaper, values: &BTreeMap<String, String>) {
        self.drafts.insert(wallpaper.favorite_key(), values.clone());
    }

    pub fn select(
        &mut self,
        wallpaper: &Wallpaper,
        assignment: Option<&Assignment>,
    ) -> BTreeMap<String, String> {
        self.search.clear();
        self.modified_only = false;
        let mut values = self
            .drafts
            .get(&wallpaper.favorite_key())
            .cloned()
            .unwrap_or_else(|| {
                assignment
                    .filter(|a| wallpaper.matches_folder(&a.wallpaper))
                    .map(|a| a.properties.clone())
                    .unwrap_or_default()
            });
        values.retain(|key, _| wallpaper.properties.get(key).is_some_and(editable));
        values
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        wallpaper: &Wallpaper,
        values: &mut BTreeMap<String, String>,
    ) {
        values.retain(|key, _| wallpaper.properties.get(key).is_some_and(editable));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Eigenschaften").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !values.is_empty(),
                        egui::Button::new("Alle zurücksetzen").small(),
                    )
                    .on_hover_text(
                        "Stellt die ursprünglichen Einstellungen wieder her. Danach anwenden.",
                    )
                    .clicked()
                {
                    values.clear();
                }
            });
        });
        ui.label(
            RichText::new("Änderungen werden beim Anwenden aktiv.")
                .small()
                .color(MUTED),
        );
        let count = wallpaper
            .properties
            .values()
            .filter(|p| editable(p))
            .count();
        if count == 0 {
            ui.add_space(12.0);
            ui.label("Dieses Wallpaper hat keine anpassbaren Optionen.");
            return;
        }
        if count > 8 {
            let label = ui.label(RichText::new("Einstellung finden").small().color(MUTED));
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Einstellungen durchsuchen …")
                    .desired_width(ui.available_width()),
            )
            .labelled_by(label.id);
        }
        if count > 8 || !values.is_empty() || self.modified_only {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.modified_only, "Nur Änderungen");
                ui.label(
                    RichText::new(format!("{} geändert", values.len()))
                        .small()
                        .color(MUTED),
                );
            });
        }
        ui.add_space(4.0);
        let query = self.search.trim().to_lowercase();
        let properties = ordered(wallpaper);
        let mut sections = vec![Section {
            group: None,
            rows: vec![],
        }];
        for (key, property) in properties {
            if kind(property) == "group" {
                sections.push(Section {
                    group: Some((key, property)),
                    rows: vec![],
                });
            } else if let Some(section) = sections.last_mut() {
                section.rows.push((key, property));
            }
        }
        let mut shown = 0;
        for Section { group, rows } in sections {
            if group.is_some_and(|(_, p)| !visible(p, wallpaper, values)) {
                continue;
            }
            let group_label = group.map(|(key, p)| label(wallpaper, key, p));
            let rows: Vec<_> = rows
                .into_iter()
                .filter(|(key, p)| {
                    visible(p, wallpaper, values)
                        && (!self.modified_only || values.contains_key(*key))
                        && (query.is_empty()
                            || label(wallpaper, key, p).to_lowercase().contains(&query)
                            || group_label
                                .as_ref()
                                .is_some_and(|s| s.to_lowercase().contains(&query)))
                })
                .collect();
            if !rows.iter().any(|(_, p)| editable(p)) {
                continue;
            }
            shown += rows.iter().filter(|(_, p)| editable(p)).count();
            let mut render = |ui: &mut egui::Ui| {
                for (key, property) in &rows {
                    ui.push_id(key, |ui| {
                        ui.scope(|ui| {
                            ui.spacing_mut().interact_size.y = 28.0;
                            ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
                            ui.spacing_mut().item_spacing.y = 6.0;
                            property_row(
                                ui,
                                wallpaper,
                                key,
                                property,
                                values,
                                &mut self.file_request,
                            );
                        });
                    });
                }
            };
            if let Some(title) = group_label {
                egui::CollapsingHeader::new(RichText::new(title).strong())
                    .id_salt((wallpaper.favorite_key(), group.map(|(key, _)| key)))
                    .default_open(true)
                    .open((!query.is_empty() || self.modified_only).then_some(true))
                    .show(ui, render);
            } else {
                render(ui);
            }
        }
        if shown == 0 {
            ui.add_space(12.0);
            ui.colored_label(
                MUTED,
                if self.modified_only {
                    "Keine geänderten Einstellungen in dieser Ansicht."
                } else {
                    "Keine passenden Einstellungen gefunden."
                },
            );
            if ui.button("Alle Einstellungen anzeigen").clicked() {
                self.search.clear();
                self.modified_only = false;
            }
        }
    }
}

fn kind(property: &Value) -> &str {
    property
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("text")
}

fn editable(property: &Value) -> bool {
    !matches!(kind(property), "text" | "group" | "divider" | "uwu" | "MC")
        && property.get("editable").and_then(Value::as_bool) != Some(false)
}

fn literal(value: &Value) -> String {
    value.as_str().map(str::to_owned).unwrap_or_else(|| {
        if value.is_null() {
            String::new()
        } else {
            value.to_string()
        }
    })
}

fn default_value(property: &Value) -> String {
    property.get("value").map(literal).unwrap_or_default()
}

fn number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
        .filter(|v| v.is_finite())
}

fn ordered(wallpaper: &Wallpaper) -> Vec<(&str, &Value)> {
    let mut properties: Vec<_> = wallpaper
        .properties
        .iter()
        .map(|(key, p)| (key.as_str(), p))
        .collect();
    properties.sort_by(|(a, ap), (b, bp)| {
        let rank = |p: &Value| {
            number(p.get("order"))
                .or_else(|| number(p.get("index")))
                .unwrap_or(f64::MAX)
        };
        rank(ap).total_cmp(&rank(bp)).then_with(|| a.cmp(b))
    });
    properties
}

/// Wallpaper labels contain HTML for headings and advertising. Keep plain text;
/// never load an author's remote images or execute their markup.
fn plain_text(source: &str) -> String {
    let mut text = String::new();
    let mut tag = false;
    for ch in source.chars() {
        match ch {
            '<' => {
                tag = true;
                text.push(' ');
            }
            '>' if tag => {
                tag = false;
                text.push(' ');
            }
            _ if !tag => text.push(ch),
            _ => (),
        }
    }
    for (entity, replacement) in [
        ("&nbsp;", " "),
        ("&amp;", "&"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&#x27;", "'"),
    ] {
        text = text.replace(entity, replacement);
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn humanize(source: &str) -> String {
    let mut result = String::new();
    let mut lower = false;
    for ch in source.trim_start_matches("ui_").chars() {
        if ch == '_' || ch == '-' {
            result.push(' ');
            lower = false;
        } else {
            if ch.is_uppercase() && lower {
                result.push(' ');
            }
            result.extend(if result.is_empty() {
                ch.to_uppercase().collect::<Vec<_>>()
            } else {
                vec![ch]
            });
            lower = ch.is_lowercase();
        }
    }
    result
}

fn localized(wallpaper: &Wallpaper, text: &str) -> String {
    let translated = ["en-us", "en"]
        .into_iter()
        .find_map(|locale| wallpaper.localization.get(locale)?.get(text)?.as_str());
    plain_text(translated.unwrap_or(match text {
        "ui_browse_properties_scheme_color" => "Scheme color",
        "ui_browse_properties_background_color" => "Background color",
        _ => text,
    }))
}

fn label(wallpaper: &Wallpaper, key: &str, property: &Value) -> String {
    let text = localized(
        wallpaper,
        property.get("text").and_then(Value::as_str).unwrap_or(key),
    );
    if text.starts_with("ui_") {
        humanize(&text)
    } else if text.is_empty() {
        humanize(key)
    } else {
        text
    }
}

fn store(values: &mut BTreeMap<String, String>, key: &str, property: &Value, value: String) {
    let default = default_value(property);
    let same = match kind(property) {
        "slider" => value
            .parse::<f64>()
            .ok()
            .zip(default.parse::<f64>().ok())
            .is_some_and(|(a, b)| (a - b).abs() < 1e-9),
        "bool" => bool_value(&value) == bool_value(&default),
        "color" => rgb(&value)
            .zip(rgb(&default))
            .is_some_and(|(a, b)| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.00001)),
        _ => value == default,
    };
    if same {
        values.remove(key);
    } else {
        values.insert(key.to_owned(), value);
    }
}

fn bool_value(value: &str) -> bool {
    value == "true" || value == "1"
}

fn rgb(value: &str) -> Option<[f32; 3]> {
    let parts: Vec<f32> = value
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    (parts.len() == 3
        && parts
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v)))
    .then(|| [parts[0], parts[1], parts[2]])
}

fn property_row(
    ui: &mut egui::Ui,
    wallpaper: &Wallpaper,
    key: &str,
    property: &Value,
    values: &mut BTreeMap<String, String>,
    file_request: &mut Option<(String, bool)>,
) {
    let property_kind = kind(property);
    if property_kind == "divider" {
        ui.separator();
        return;
    }
    if !editable(property) {
        let text = property
            .get("text")
            .and_then(Value::as_str)
            .map(|s| localized(wallpaper, s))
            .unwrap_or_default();
        if !text.is_empty() {
            ui.add_space(8.0);
            ui.label(RichText::new(text).small().color(MUTED));
        }
        return;
    }
    ui.add_space(8.0);
    let title = label(wallpaper, key, property);
    let mut value = values
        .get(key)
        .cloned()
        .unwrap_or_else(|| default_value(property));
    let original = value.clone();
    let mut reset = false;
    let mut label_id = egui::Id::NULL;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            reset = ui
                .add_enabled(
                    values.contains_key(key),
                    egui::Button::new("Zurücksetzen").small(),
                )
                .on_hover_text(format!(
                    "Standard wiederherstellen: {}",
                    default_value(property)
                ))
                .clicked();
            ui.with_layout(
                egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                |ui| {
                    if property_kind == "bool" {
                        let mut enabled = bool_value(&value);
                        if ui.checkbox(&mut enabled, &title).changed() {
                            value = enabled.to_string();
                        }
                    } else {
                        label_id = ui
                            .add(egui::Label::new(RichText::new(&title).strong()).wrap())
                            .id;
                    }
                },
            );
        });
    });
    match property_kind {
        "bool" => {}
        "slider" => {
            let min = number(property.get("min")).unwrap_or(0.0);
            let max = number(property.get("max")).unwrap_or(100.0).max(min);
            let mut numeric = value
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .unwrap_or(min);
            let precision = property
                .get("precision")
                .and_then(Value::as_u64)
                .map(|v| (v as usize).min(6))
                .unwrap_or(
                    if property.get("fraction").and_then(Value::as_bool) == Some(false) {
                        0
                    } else {
                        2
                    },
                );
            ui.scope(|ui| {
                ui.spacing_mut().slider_width = (ui.available_width() - 92.0).max(60.0);
                let mut slider = egui::Slider::new(&mut numeric, min..=max).max_decimals(precision);
                if let Some(step) = number(property.get("step")).filter(|s| *s > 0.0) {
                    slider = slider.step_by(step);
                }
                if precision == 0 {
                    slider = slider.integer();
                }
                if super::theme::slider(ui, slider)
                    .labelled_by(label_id)
                    .changed()
                {
                    value = numeric.to_string();
                }
            });
        }
        "combo" => {
            let options = property
                .get("options")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let option_label = |p: &Value| {
                localized(
                    wallpaper,
                    p.get("label")
                        .or_else(|| p.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or("Option"),
                )
            };
            let selected = options
                .iter()
                .find(|p| p.get("value").is_some_and(|v| literal(v) == value))
                .map(option_label)
                .unwrap_or_else(|| {
                    if value.is_empty() {
                        "Option auswählen".into()
                    } else {
                        value.clone()
                    }
                });
            egui::ComboBox::from_id_salt("value")
                .width((ui.available_width() - 12.0).max(60.0))
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for option in options {
                        if let Some(v) = option.get("value") {
                            ui.selectable_value(&mut value, literal(v), option_label(option));
                        }
                    }
                })
                .response
                .labelled_by(label_id);
        }
        "color" if rgb(&value).is_some() => {
            let mut color = rgb(&value)
                .unwrap_or([1.0; 3])
                .map(|v| (v * 255.0).round() as u8);
            ui.horizontal(|ui| {
                if ui
                    .color_edit_button_srgb(&mut color)
                    .labelled_by(label_id)
                    .on_hover_text("Farbe auswählen")
                    .changed()
                {
                    value = color
                        .iter()
                        .map(|c| format!("{:.6}", f32::from(*c) / 255.0))
                        .collect::<Vec<_>>()
                        .join(" ");
                }
                ui.label(
                    RichText::new(format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2]))
                        .monospace(),
                );
                ui.label(RichText::new("Zum Bearbeiten klicken").small().color(MUTED));
            });
        }
        _ => {
            ui.add(
                egui::TextEdit::singleline(&mut value)
                    .desired_width(ui.available_width())
                    .hint_text(
                        if matches!(
                            property_kind,
                            "file" | "directory" | "scenetexture" | "texture"
                        ) {
                            "Datei- oder Ordnerpfad"
                        } else {
                            "Wert eingeben"
                        },
                    ),
            )
            .labelled_by(label_id);
            if matches!(
                property_kind,
                "file" | "directory" | "scenetexture" | "texture"
            ) && ui
                .button(if property_kind == "directory" {
                    "Ordner auswählen …"
                } else {
                    "Datei auswählen …"
                })
                .clicked()
            {
                *file_request = Some((key.to_owned(), property_kind == "directory"));
            }
            if !matches!(
                property_kind,
                "textinput" | "file" | "directory" | "scenetexture" | "texture"
            ) {
                ui.label(
                    RichText::new(format!("Erweiterter Wert · {property_kind}"))
                        .small()
                        .color(MUTED),
                );
            }
        }
    }
    if reset {
        values.remove(key);
    } else if value != original {
        store(values, key, property, value);
    }
    ui.add_space(4.0);
}

fn visible(property: &Value, wallpaper: &Wallpaper, values: &BTreeMap<String, String>) -> bool {
    let condition = property
        .get("condition")
        .and_then(Value::as_str)
        .unwrap_or_default();
    condition.trim().is_empty()
        || condition_value(condition, wallpaper, values, 0).is_none_or(|v| truthy(&v))
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(v) => *v,
        Value::Number(_) => v.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Null => false,
        _ => true,
    }
}

/// A deliberately small expression reader for author-provided display conditions.
/// Unknown syntax leaves the setting visible. No JavaScript or code is executed.
fn condition_value(
    source: &str,
    wallpaper: &Wallpaper,
    values: &BTreeMap<String, String>,
    depth: usize,
) -> Option<Value> {
    if depth > 24 || source.len() > 4096 {
        return None;
    }
    let source = source.trim();
    let mut nesting = 0_i32;
    let mut quote = None;
    let mut escaped = false;
    let mut operators = vec![];
    for (i, ch) in source.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            continue;
        }
        if ch == '(' {
            nesting += 1;
        }
        if ch == ')' {
            nesting -= 1;
            if nesting < 0 {
                return None;
            }
        }
        if nesting == 0 {
            operators.push(i);
        }
    }
    if nesting != 0 || quote.is_some() {
        return None;
    }
    for ops in [
        &["||"][..],
        &["&&"][..],
        &["===", "!==", "==", "!=", "<=", ">=", "<", ">"][..],
    ] {
        for &i in &operators {
            if i == 0 {
                continue;
            }
            if let Some(op) = ops.iter().find(|op| source[i..].starts_with(**op)) {
                let a = condition_value(&source[..i], wallpaper, values, depth + 1)?;
                let b = condition_value(&source[i + op.len()..], wallpaper, values, depth + 1)?;
                let numeric = |v: &Value| {
                    v.as_f64()
                        .or_else(|| v.as_str()?.parse::<f64>().ok())
                        .or_else(|| v.as_bool().map(|b| if b { 1.0 } else { 0.0 }))
                };
                let equal = a == b || numeric(&a).zip(numeric(&b)).is_some_and(|(a, b)| a == b);
                let strict_equal = a == b
                    || (a.is_number()
                        && b.is_number()
                        && a.as_f64().zip(b.as_f64()).is_some_and(|(a, b)| a == b));
                return Some(Value::Bool(match *op {
                    "||" => truthy(&a) || truthy(&b),
                    "&&" => truthy(&a) && truthy(&b),
                    "==" => equal,
                    "!=" => !equal,
                    "===" => strict_equal,
                    "!==" => !strict_equal,
                    op => {
                        let (a, b) = numeric(&a).zip(numeric(&b))?;
                        match op {
                            "<" => a < b,
                            ">" => a > b,
                            "<=" => a <= b,
                            _ => a >= b,
                        }
                    }
                }));
            }
        }
    }
    if source.starts_with('(') && source.ends_with(')') {
        return condition_value(&source[1..source.len() - 1], wallpaper, values, depth + 1);
    }
    if let Some(source) = source.strip_prefix('!') {
        return Some(Value::Bool(!truthy(&condition_value(
            source,
            wallpaper,
            values,
            depth + 1,
        )?)));
    }
    if let Some(key) = source.strip_suffix(".value") {
        let p = wallpaper.properties.get(key)?;
        let value = values.get(key).cloned().unwrap_or_else(|| default_value(p));
        return match kind(p) {
            "bool" => Some(Value::Bool(bool_value(&value))),
            "slider" => value
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number),
            _ => {
                // Combo values may be strings, numbers or booleans; preserve
                // their declared type when comparing a display condition.
                let declared = p
                    .get("options")
                    .and_then(Value::as_array)
                    .and_then(|options| {
                        options
                            .iter()
                            .filter_map(|o| o.get("value"))
                            .find(|v| literal(v) == value)
                    })
                    .or_else(|| p.get("value"));
                if declared.is_some_and(|v| v.is_number() || v.is_boolean()) {
                    serde_json::from_str(&value).ok()
                } else {
                    Some(Value::String(value))
                }
            }
        };
    }
    if source.starts_with('\'') && source.ends_with('\'') && source.len() >= 2 {
        return Some(Value::String(source[1..source.len() - 1].into()));
    }
    serde_json::from_str(source).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Wallpaper {
        Wallpaper {
            id: "123".into(), title: "Customizable wallpaper".into(), kind: "web".into(),
            folder: "/fixture/steamapps/workshop/content/431960/123".into(), file: "index.html".into(),
            packed: false, staged: false, preview: None,
            properties: serde_json::from_value(json!({
                "enabled": {"type":"bool","text":"Show clock","value":false,"order":1},
                "speed": {"type":"slider","text":"Animation speed","value":"1.5","min":0,"max":4,"order":2.5},
                "mode": {"type":"combo","text":"Clock style","value":0,"order":3,"options":[{"label":"Digital","value":0},{"label":"Analog","value":1}]},
                "message": {"type":"textinput","text":"Clock caption","value":"Hello","order":4},
                "color": {"type":"color","text":"ui_color","value":"0.2 0.4 0.6","order":5},
                "image": {"type":"file","text":"Background image","order":6},
                "folder": {"type":"directory","text":"Slideshow folder","order":7},
                "group": {"type":"group","text":"<b>More options</b>","order":8},
                "child": {"type":"slider","text":"Clock size","value":2,"min":1,"max":4,"condition":"enabled.value && mode.value === 1","order":9},
                "note": {"type":"text","text":"<b>Author note</b>","order":10}
            })).expect("properties"),
            localization: json!({"en-us":{"ui_color":"<b>Accent &amp; glow</b>"}}),
        }
    }

    #[test]
    fn author_order_localization_and_markup_are_respected() {
        let w = fixture();
        let keys: Vec<_> = ordered(&w).into_iter().map(|(key, _)| key).collect();
        assert_eq!(
            keys,
            [
                "enabled", "speed", "mode", "message", "color", "image", "folder", "group",
                "child", "note"
            ]
        );
        assert_eq!(label(&w, "color", &w.properties["color"]), "Accent & glow");
        assert_eq!(
            plain_text("<hr><center><img src='https://example.test/a'></center>"),
            ""
        );
        assert_eq!(
            plain_text("背景<br/>Background <b>image</b>"),
            "背景 Background image"
        );
        assert!(!editable(&w.properties["note"]));
        assert!(!editable(&w.properties["group"]));
    }

    #[test]
    fn conditions_follow_drafts_precedence_and_declared_combo_types() {
        let w = fixture();
        let mut values = BTreeMap::new();
        assert!(!visible(&w.properties["child"], &w, &values));
        values.insert("enabled".into(), "true".into());
        values.insert("mode".into(), "1".into());
        assert!(visible(&w.properties["child"], &w, &values));
        for (condition, expected) in [
            ("!enabled.value", false),
            (
                "(mode.value === 0 || mode.value === 1) && enabled.value",
                true,
            ),
            ("speed.value >= 1 && !(mode.value == 0)", true),
            ("false || true && false", false),
            ("message.value == 'Hello'", true),
            ("message.value == 'Hello && World'", false),
            ("mode.value !== '1'", true),
            ("mode.value == '1'", true),
            ("mode.value === 1.0", true),
            ("speed.value === 1.5", true),
        ] {
            assert_eq!(
                condition_value(condition, &w, &values, 0).map(|v| truthy(&v)),
                Some(expected),
                "{condition}"
            );
        }
        // Unsupported author expressions fail open instead of hiding controls.
        for condition in [
            "unknown.value",
            "window.doSomething()",
            "enabled.value ? true : false",
            "((((",
            "value==ture",
        ] {
            assert!(
                visible(&json!({"condition":condition}), &w, &values),
                "{condition}"
            );
        }
    }

    #[test]
    fn overrides_preserve_empty_text_and_normalize_default_values() {
        let w = fixture();
        let mut values = BTreeMap::new();
        for (key, value) in [
            ("enabled", "0"),
            ("speed", "1.500"),
            ("color", "0.200000 0.400000 0.600000"),
        ] {
            store(&mut values, key, &w.properties[key], value.into());
        }
        assert!(values.is_empty());
        store(
            &mut values,
            "message",
            &w.properties["message"],
            String::new(),
        );
        store(
            &mut values,
            "enabled",
            &w.properties["enabled"],
            "true".into(),
        );
        assert_eq!(values["message"], "");
        assert_eq!(values["enabled"], "true");
        store(
            &mut values,
            "message",
            &w.properties["message"],
            "Hello".into(),
        );
        assert!(!values.contains_key("message"));
    }

    #[test]
    fn selection_restores_assignments_and_keeps_drafts_per_wallpaper() {
        let w = fixture();
        let mut editor = PropertiesUi::default();
        let assignment = Assignment {
            wallpaper: w.folder.clone(),
            properties: BTreeMap::from([
                ("speed".into(), "2".into()),
                ("removed".into(), "bad".into()),
            ]),
        };
        let mut values = editor.select(&w, Some(&assignment));
        assert_eq!(values.len(), 1);
        values.insert("message".into(), String::new());
        editor.remember(&w, &values);
        let mut other = w.clone();
        other.id = "456".into();
        other.folder = "/fixture/steamapps/workshop/content/431960/456".into();
        assert!(editor.select(&other, Some(&assignment)).is_empty());
        assert_eq!(editor.select(&w, None), values);
        let mut moved = w.clone();
        moved.folder = "/fixture/steamapps/workshop/downloads/431960/123".into();
        assert_eq!(editor.select(&moved, None), values);
    }

    fn text_position(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(t) = &shape.shape
                    && t.galley.text() == text
                {
                    return Some(t.pos + t.galley.size() * 0.5);
                }
                None
            })
            .unwrap_or_else(|| panic!("Missing UI text: {text}"))
    }

    #[test]
    fn clicking_checkbox_and_reset_updates_the_renderer_overrides() {
        let ctx = egui::Context::default();
        let w = fixture();
        let mut editor = PropertiesUi::default();
        let mut values = BTreeMap::new();
        let mut render = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(336.0, 1800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| editor.show(ui, &w, &mut values));
                },
            )
        };
        let initial = render(vec![]);
        let mut click = |position: egui::Pos2| {
            for pressed in [true, false] {
                render(vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
            }
            render(vec![])
        };
        let changed = click(text_position(&initial, "Show clock"));
        assert!(text_position(&changed, "1 geändert").is_finite());
        // The changed-only filter appears after the first edit; use fresh geometry.
        let restored = click(text_position(&changed, "Zurücksetzen"));
        assert!(text_position(&restored, "Show clock").is_finite());
        assert!(values.is_empty());
    }

    #[test]
    fn apply_action_stays_in_view_with_many_settings_at_supported_window_sizes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = egui::Context::default();
        let mut app = super::super::App::new(
            crate::config::Paths {
                config: dir.path().join("config.toml"),
                state: dir.path().join("state"),
                runtime: dir.path().join("run"),
            },
            &ctx,
        );
        let mut w = fixture();
        for i in 0..100 {
            w.properties.insert(format!("extra{i}"), json!({"type":"slider","text":format!("Extra setting {i}"),"value":1,"min":0,"max":10}));
        }
        app.scan.wallpapers.push(w.clone());
        app.select(&w, &ctx);
        app.output = "DP-1".into();
        for (width, height) in [(960.0, 640.0), (1360.0, 840.0)] {
            // Native panels measure their first layout before painting content.
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, height),
                    )),
                    ..Default::default()
                },
                |ctx| app.draw(ctx),
            );
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
            let position = text_position(&output, "Auf DP-1 anwenden");
            assert!(position.y < height - 40.0 && position.y > height * 0.5);
            assert!(position.x < width);
        }
    }

    #[test]
    fn typed_controls_fit_narrow_and_wide_inspectors_without_changing_defaults() {
        let ctx = egui::Context::default();
        let w = fixture();
        for width in [280.0, 336.0, 452.0] {
            let mut editor = PropertiesUi::default();
            let mut values = BTreeMap::new();
            let mut used_width = 0.0_f32;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 1800.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        editor.show(ui, &w, &mut values);
                        used_width = ui.min_rect().width();
                    });
                },
            );
            assert!(!output.shapes.is_empty());
            assert!(used_width <= width, "{used_width} exceeds {width}");
            assert!(values.is_empty(), "Rendering must not create overrides");
        }
    }
}
