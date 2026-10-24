use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

/// Keep Latin text in the usual UI fonts, and supply CJK glyphs from a bundled
/// font instead of relying on fonts installed on the user's machine.
pub(crate) fn configure(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let mut data = FontData::from_static(include_bytes!("../assets/fonts/NotoSansCJK-Regular.ttc"));
    data.index = 2; // Noto Sans CJK SC within the original, unmodified collection.
    let name = "wallweave-cjk".to_owned();
    fonts.font_data.insert(name.clone(), Arc::new(data));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push(name.clone());
    }
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workshop_titles_have_real_cjk_glyphs_in_both_ui_font_families() {
        let ctx = egui::Context::default();
        configure(&ctx);
        let _ = ctx.run(Default::default(), |ctx| {
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                let font = egui::FontId::new(15.0, family);
                ctx.fonts(|fonts| {
                    for glyph in
                        "中文简体繁體壁纸桌布星瞳藏刃飞花入梦無背景音楽日本語한글äöü".chars()
                    {
                        assert!(fonts.has_glyph(&font, glyph), "Missing glyph: {glyph}");
                    }
                });
            }
        });
    }
}
