#![windows_subsystem = "windows"]

mod app;
mod client;
mod i18n;
mod log;
mod model;
mod pages;
mod preferences;
mod skin;

use eframe::egui;
use preferences::ThemePreference;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MzLauncher")
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/logo-pixel-256.png"))
                    .expect("application icon must be a valid PNG"),
            )
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "MzLauncher",
        options,
        Box::new(|cc| {
            install_cjk_fonts(&cc.egui_ctx);
            Ok(Box::new(app::MzApp::new(cc)))
        }),
    )
}

pub fn apply_theme(ctx: &egui::Context, theme: ThemePreference, system_dark: bool) {
    let dark = match theme {
        ThemePreference::Light => false,
        ThemePreference::Dark => true,
        ThemePreference::System => system_dark,
    };
    let mut style = (*ctx.style()).clone();
    style.visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    style.visuals.panel_fill = if dark {
        egui::Color32::from_rgb(24, 27, 32)
    } else {
        egui::Color32::from_rgb(243, 245, 248)
    };
    style.visuals.window_fill = if dark {
        egui::Color32::from_rgb(32, 36, 43)
    } else {
        egui::Color32::WHITE
    };
    style.visuals.extreme_bg_color = if dark {
        egui::Color32::from_rgb(16, 18, 22)
    } else {
        egui::Color32::from_rgb(232, 235, 240)
    };
    style.visuals.widgets.noninteractive.bg_fill = style.visuals.window_fill;
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(
        1.0_f32,
        if dark {
            egui::Color32::from_rgb(55, 61, 70)
        } else {
            egui::Color32::from_rgb(222, 226, 232)
        },
    );
    style.visuals.selection.bg_fill = egui::Color32::from_rgb(59, 130, 246);
    style.visuals.hyperlink_color = egui::Color32::from_rgb(59, 130, 246);
    for widget in [
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(7);
    }
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    ctx.set_style(style);
}

pub fn card_frame(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(14))
}

fn install_cjk_fonts(ctx: &egui::Context) {
    let candidates: &[&str] = if cfg!(windows) {
        &[
            "C:/Windows/Fonts/msyh.ttc",
            "C:/Windows/Fonts/simhei.ttf",
            "C:/Windows/Fonts/simsun.ttc",
        ]
    } else if cfg!(target_os = "macos") {
        &["/System/Library/Fonts/PingFang.ttc"]
    } else {
        &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        ]
    };
    let Some(bytes) = candidates.iter().find_map(|path| std::fs::read(path).ok()) else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".into(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("cjk".into());
    }
    ctx.set_fonts(fonts);
}
