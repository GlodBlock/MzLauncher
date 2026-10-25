use eframe::egui;
use serde_json::{json, Value};

use crate::app::MzApp;
use crate::pages::home::loader_label;

impl MzApp {
    pub fn modpack_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("导入整合包");
        ui.label(
            "支持 CurseForge 与 Modrinth 本地整合包（.zip / .mrpack），将导入到新的隔离实例。",
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("整合包文件：");
            ui.add(
                egui::TextEdit::singleline(&mut self.modpack.path)
                    .desired_width(420.0)
                    .hint_text("选择本地整合包文件"),
            );
            if ui.button("浏览…").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("整合包", &["zip", "mrpack"])
                    .pick_file()
                {
                    self.modpack.path = path.display().to_string();
                    self.modpack.analysis = None;
                }
            }
            if ui
                .add_enabled(
                    !self.modpack.path.trim().is_empty() && !self.modpack.analyzing,
                    egui::Button::new("分析"),
                )
                .clicked()
            {
                self.modpack.analyzing = true;
                self.modpack.analysis = None;
                self.call(
                    "modpack.analyze",
                    json!({"path": self.modpack.path.trim()}),
                    Value::Null,
                );
            }
            if self.modpack.analyzing {
                ui.spinner();
            }
        });
        let Some(analysis) = self.modpack.analysis.clone() else {
            return;
        };
        ui.separator();
        ui.heading("预览");
        ui.label(format!("格式：{}", analysis.format));
        ui.label(format!(
            "名称：{} {}",
            analysis.name,
            analysis.version.unwrap_or_default()
        ));
        if let Some(author) = analysis.author {
            ui.label(format!("作者：{author}"));
        }
        ui.label(format!(
            "游戏版本：{} · {} {}",
            analysis.game_version,
            loader_label(&analysis.loader),
            analysis.loader_version.unwrap_or_default()
        ));
        ui.label(format!("Mod 数量：{}", analysis.mod_count));
        if !analysis.override_prefixes.is_empty() {
            ui.label(format!(
                "将应用覆盖目录：{}",
                analysis.override_prefixes.join(", ")
            ));
        }
        if !analysis.warnings.is_empty() {
            ui.colored_label(
                egui::Color32::from_rgb(200, 140, 40),
                "以下条目需要手动处理：",
            );
            for warning in &analysis.warnings {
                ui.label(format!("- {warning}"));
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("新实例名称：");
            ui.text_edit_singleline(&mut self.modpack.name);
        });
        if ui.button("开始导入").clicked() {
            let name = self.modpack.name.trim();
            self.call(
                "modpack.import",
                json!({
                    "path": self.modpack.path.trim(),
                    "name": if name.is_empty() { Value::Null } else { json!(name) },
                }),
                Value::Null,
            );
        }
    }
}
