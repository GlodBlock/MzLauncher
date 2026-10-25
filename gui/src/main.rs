#![windows_subsystem = "windows"]

mod app;
mod client;
mod model;
mod pages;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MzLauncher")
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
