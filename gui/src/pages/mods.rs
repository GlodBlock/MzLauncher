use eframe::egui;
use serde_json::{json, Value};

use crate::app::MzApp;
use crate::pages::home::loader_key;

impl MzApp {
    pub fn mods_page(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.tr("browser.title"));
        ui.horizontal_wrapped(|ui| {
            ui.label(self.tr("browser.provider"));
            ui.selectable_value(
                &mut self.browser.provider,
                "modrinth".to_owned(),
                "Modrinth",
            );
            ui.selectable_value(
                &mut self.browser.provider,
                "curse_forge".to_owned(),
                "CurseForge",
            );
            ui.label(self.tr("browser.target"));
            let target_name = self
                .browser
                .target
                .as_deref()
                .map(|id| self.instance_name(id))
                .unwrap_or_else(|| self.tr("browser.no_target"));
            egui::ComboBox::from_id_salt("mod_target")
                .selected_text(target_name)
                .show_ui(ui, |ui| {
                    for instance in self.instances.clone() {
                        ui.selectable_value(
                            &mut self.browser.target,
                            Some(instance.id.clone()),
                            &instance.name,
                        );
                    }
                });
        });
        ui.horizontal(|ui| {
            let search_hint = self.tr("browser.search_hint");
            let spacing = ui.spacing().item_spacing.x;
            let spinner_width = if self.browser.searching {
                ui.spacing().interact_size.y + spacing
            } else {
                0.0
            };
            let search_width = (ui.available_width()
                - crate::layout::TOOLBAR_CONTROL_SIZE.x
                - spacing
                - spinner_width)
                .max(0.0);
            crate::widgets::search_field(ui, &mut self.browser.query, search_hint, search_width);
            if crate::widgets::search_button(ui, &self.tr("browser.search"), true).clicked() {
                self.search_mods();
            }
            if self.browser.searching {
                ui.spinner();
            }
        });
        ui.separator();
        let target = self.browser.target.clone();
        let results = self.browser.results.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            if results.is_empty() && !self.browser.searching {
                ui.label(self.tr("browser.empty"));
            }
            for project in results {
                crate::card_frame(ui).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(&project.name);
                        ui.label(if project.provider == "curse_forge" {
                            "CurseForge"
                        } else {
                            "Modrinth"
                        });
                        ui.label(self.trf("browser.author", &[("author", &project.author)]));
                        ui.label(self.trf(
                            "browser.downloads",
                            &[("count", &project.downloads.to_string())],
                        ));
                        if project.page_url.is_some()
                            && ui.small_button(self.tr("browser.open_page")).clicked()
                        {
                            let _ = open::that(project.page_url.clone().unwrap());
                        }
                    });
                    ui.add(egui::Label::new(&project.summary).wrap());
                    let expanded =
                        self.browser.expanded.as_deref() == Some(project.project_id.as_str());
                    if ui
                        .button(self.tr(if expanded {
                            "browser.hide_files"
                        } else {
                            "browser.show_files"
                        }))
                        .clicked()
                    {
                        if expanded {
                            self.browser.expanded = None;
                        } else {
                            self.browser.expanded = Some(project.project_id.clone());
                            if let Some(target) = &target {
                                self.call(
                                    "mod.files",
                                    json!({
                                        "provider": self.browser.provider,
                                        "instanceId": target,
                                        "projectId": project.project_id,
                                    }),
                                    json!({"projectId": project.project_id}),
                                );
                            }
                        }
                    }
                    if expanded {
                        let files = self
                            .browser
                            .files
                            .get(&project.project_id)
                            .cloned()
                            .unwrap_or_default();
                        if files.is_empty() {
                            ui.label(self.tr("browser.files_empty"));
                        }
                        for file in files {
                            ui.horizontal_wrapped(|ui| {
                                ui.add(
                                    egui::Label::new(format!(
                                        "{} · {} · {}",
                                        file.version_name, file.channel, file.file_name
                                    ))
                                    .wrap(),
                                );
                                let loaders = file
                                    .loaders
                                    .iter()
                                    .map(|loader| self.tr(loader_key(loader)))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(self.trf(
                                            "browser.file_support",
                                            &[
                                                ("versions", &file.game_versions.join(", ")),
                                                ("loaders", &loaders),
                                            ],
                                        ))
                                        .small(),
                                    )
                                    .wrap(),
                                );
                                if !file.distribution_allowed {
                                    ui.label(self.tr("browser.manual_download"));
                                } else if let Some(target) = &target {
                                    if ui.small_button(self.tr("browser.install")).clicked() {
                                        self.call(
                                            "mod.install",
                                            json!({"instanceId": target, "file": file}),
                                            json!({"instanceId": target}),
                                        );
                                    }
                                }
                            });
                        }
                    }
                });
                ui.add_space(crate::layout::CARD_LIST_GAP);
            }
        });
    }

    fn search_mods(&mut self) {
        let target = self.browser.target.clone();
        let instance = target.as_deref().and_then(|id| self.instance(id)).cloned();
        self.browser.searching = true;
        self.browser.expanded = None;
        self.call(
            "mod.search",
            json!({
                "provider": self.browser.provider,
                "query": self.browser.query,
                "gameVersion": instance.as_ref().map(|instance| instance.game_version.clone()),
                "loader": instance.map(|instance| instance.loader),
            }),
            Value::Null,
        );
    }
}
