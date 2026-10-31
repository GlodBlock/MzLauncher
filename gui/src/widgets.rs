use eframe::egui;

const TOOLBAR_ICON_SIZE: egui::Vec2 = egui::Vec2::splat(18.0);

pub fn themed_image(
    ui: &egui::Ui,
    name: &str,
    light_png: &'static [u8],
    dark_png: &'static [u8],
    size: egui::Vec2,
) -> egui::Image<'static> {
    let (theme, png) = if ui.visuals().dark_mode {
        ("dark", dark_png)
    } else {
        ("light", light_png)
    };
    egui::Image::from_bytes(format!("bytes://{name}-{theme}.png"), png).fit_to_exact_size(size)
}

pub struct IconButton<'a> {
    name: &'a str,
    light_png: &'static [u8],
    dark_png: &'static [u8],
    enabled: bool,
}

impl<'a> IconButton<'a> {
    pub fn new(name: &'a str, light_png: &'static [u8], dark_png: &'static [u8]) -> Self {
        Self {
            name,
            light_png,
            dark_png,
            enabled: true,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn show(self, ui: &mut egui::Ui, tooltip: &str) -> egui::Response {
        let image = themed_image(
            ui,
            self.name,
            self.light_png,
            self.dark_png,
            TOOLBAR_ICON_SIZE,
        );
        ui.add_enabled(
            self.enabled,
            egui::Button::image(image)
                .min_size(crate::layout::TOOLBAR_CONTROL_SIZE)
                .corner_radius(egui::CornerRadius::same(7)),
        )
        .on_hover_text(tooltip)
    }
}

pub fn search_field(
    ui: &mut egui::Ui,
    value: &mut String,
    hint: impl Into<egui::WidgetText>,
    width: f32,
) -> egui::Response {
    ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(width)
            .min_size(egui::vec2(width, crate::layout::TOOLBAR_CONTROL_SIZE.y))
            .vertical_align(egui::Align::Center)
            .hint_text(hint),
    )
}

pub fn search_button(ui: &mut egui::Ui, tooltip: &str, enabled: bool) -> egui::Response {
    IconButton::new(
        "toolbar-search",
        include_bytes!("../assets/search-light.png"),
        include_bytes!("../assets/search-dark.png"),
    )
    .enabled(enabled)
    .show(ui, tooltip)
}

pub fn themed_progress_bar(
    ui: &mut egui::Ui,
    fraction: f32,
    width: f32,
    text: impl Into<String>,
) -> egui::Response {
    let fraction = fraction.clamp(0.0, 1.0);
    let text = text.into();
    let (fill, filled_text_color) = if ui.visuals().dark_mode {
        (
            egui::Color32::from_rgb(243, 244, 246),
            egui::Color32::from_rgb(32, 36, 43),
        )
    } else {
        (
            egui::Color32::from_rgb(32, 36, 43),
            egui::Color32::from_rgb(243, 244, 246),
        )
    };
    let response = ui.add(
        egui::ProgressBar::new(fraction)
            .desired_width(width)
            .fill(fill),
    );
    let rect = response.rect;
    let font_id = egui::TextStyle::Body.resolve(ui.style());
    ui.painter().with_clip_rect(rect).text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text.clone(),
        font_id.clone(),
        ui.visuals().text_color(),
    );
    let filled_rect = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(rect.min.x + rect.width() * fraction, rect.max.y),
    );
    ui.painter().with_clip_rect(filled_rect).text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        font_id,
        filled_text_color,
    );
    response
}
