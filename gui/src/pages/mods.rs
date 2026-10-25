use eframe::egui;
use serde_json::{json, Value};

use crate::app::MzApp;
use crate::pages::home::loader_label;

impl MzApp {
    pub fn mods_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mod 浏览");
        ui.horizontal(|ui| {
            ui.label("来源：");
            for (value, label) in [("modrinth", "Modrinth"), ("curse_forge", "CurseForge")] {
                ui.selectable_value(&mut self.browser.provider, value.to_owned(), label);
            }
            ui.label("目标实例：");
            let target_name = self
                .browser
                .target
                .as_deref()
                .map(|id| self.instance_name(id))
                .unwrap_or_else(|| "无可用实例".into());
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
            ui.add(
                egui::TextEdit::singleline(&mut self.browser.query)
                    .desired_width(320.0)
                    .hint_text("输入名称或关键词"),
            );
            if ui.button("搜索").clicked() {
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
                ui.label("输入关键词开始搜索。安装前请先选择目标实例。");
            }
            for project in results {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong(&project.name);
                        ui.label(if project.provider == "curse_forge" {
                            "CurseForge"
                        } else {
                            "Modrinth"
                        });
                        ui.label(format!("作者：{}", project.author));
                        ui.label(format!("下载量：{}", project.downloads));
                        if project.page_url.is_some() && ui.small_button("打开来源页").clicked()
                        {
                            let _ = open::that(project.page_url.clone().unwrap());
                        }
                    });
                    ui.label(&project.summary);
                    let expanded =
                        self.browser.expanded.as_deref() == Some(project.project_id.as_str());
                    if ui
                        .button(if expanded {
                            "收起版本"
                        } else {
                            "选择版本"
                        })
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
                            ui.label("正在加载版本，或该实例版本/加载器下没有可用文件。");
                        }
                        for file in files {
                            ui.horizontal(|ui| {
                                ui.label(format!(
                                    "{} · {} · {}",
                                    file.version_name, file.channel, file.file_name
                                ));
                                ui.small(format!(
                                    "支持：{} {}",
                                    file.game_versions.join(", "),
                                    file.loaders
                                        .iter()
                                        .map(|loader| loader_label(loader))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ));
                                if !file.distribution_allowed {
                                    ui.label("需手动下载");
                                } else if let Some(target) = &target {
                                    if ui.small_button("安装").clicked() {
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
                ui.add_space(4.0);
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
