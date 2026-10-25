use eframe::egui;
use serde_json::{json, Value};

use crate::app::MzApp;
use crate::pages::home::loader_key;

impl MzApp {
    pub fn modpack_page(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.tr("modpack.title"));
        ui.label(self.tr("modpack.description"));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(self.tr("modpack.file"));
            let path_hint = self.tr("modpack.path_hint");
            ui.add(
                egui::TextEdit::singleline(&mut self.modpack.path)
                    .desired_width(420.0)
                    .hint_text(path_hint),
            );
            if ui.button(self.tr("modpack.browse")).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Modpack", &["zip", "mrpack"])
                    .pick_file()
                {
                    self.modpack.path = path.display().to_string();
                    self.modpack.analysis = None;
                }
            }
            if ui
                .add_enabled(
                    !self.modpack.path.trim().is_empty() && !self.modpack.analyzing,
                    egui::Button::new(self.tr("modpack.analyze")),
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
        ui.heading(self.tr("modpack.preview"));
        ui.label(self.trf("modpack.format", &[("format", &analysis.format)]));
        ui.label(self.trf(
            "modpack.name_version",
            &[
                ("name", &analysis.name),
                ("version", &analysis.version.unwrap_or_default()),
            ],
        ));
        if let Some(author) = analysis.author {
            ui.label(self.trf("modpack.author", &[("author", &author)]));
        }
        ui.label(self.trf(
            "modpack.game",
            &[
                ("version", &analysis.game_version),
                ("loader", &self.tr(loader_key(&analysis.loader))),
                (
                    "loaderVersion",
                    &analysis.loader_version.unwrap_or_default(),
                ),
            ],
        ));
        ui.label(self.trf(
            "modpack.mod_count",
            &[("count", &analysis.mod_count.to_string())],
        ));
        if !analysis.override_prefixes.is_empty() {
            ui.label(self.trf(
                "modpack.overrides",
                &[("prefixes", &analysis.override_prefixes.join(", "))],
            ));
        }
        if !analysis.warnings.is_empty() {
            ui.colored_label(
                egui::Color32::from_rgb(200, 140, 40),
                self.tr("modpack.warnings"),
            );
            for warning in &analysis.warnings {
                ui.label(format!("- {warning}"));
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(self.tr("modpack.instance_name"));
            ui.text_edit_singleline(&mut self.modpack.name);
        });
        if ui.button(self.tr("modpack.import")).clicked() {
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
