use eframe::egui;

pub const ITEM_SPACING: egui::Vec2 = egui::Vec2::new(10.0, 8.0);
pub const BUTTON_PADDING: egui::Vec2 = egui::Vec2::new(12.0, 6.0);
pub const CARD_INNER_MARGIN: i8 = 14;
pub const CARD_LIST_GAP: f32 = ITEM_SPACING.y;
pub const SECTION_GAP: f32 = 12.0;
pub const TOOLBAR_CONTROL_SIZE: egui::Vec2 = egui::Vec2::splat(42.0);
pub const CONTENT_TRAILING_RESERVE: f32 = 16.0;

pub fn button_width(ui: &egui::Ui, text: impl Into<String>) -> f32 {
    let font_id = egui::TextStyle::Button.resolve(ui.style());
    let text_width = ui.fonts(|fonts| {
        fonts
            .layout_no_wrap(text.into(), font_id, egui::Color32::WHITE)
            .size()
            .x
    });
    text_width + ui.spacing().button_padding.x * 2.0
}
