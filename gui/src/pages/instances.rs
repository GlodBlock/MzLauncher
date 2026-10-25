use eframe::egui;
use serde_json::{json, Value};

use crate::app::{CreateDialog, DetailTab, MzApp};
use crate::model::Instance;
use crate::pages::home::{loader_label, status_label};

impl MzApp {
    pub fn open_create_dialog(&mut self) {
        self.create = Some(CreateDialog {
            name: String::new(),
            game_version: String::new(),
            loader: "vanilla".into(),
            loader_version: String::new(),
            include_snapshots: false,
        });
        self.versions.clear();
        self.call(
            "version.list",
            json!({"includeSnapshots": false}),
            Value::Null,
        );
    }

    pub fn instances_page(&mut self, ui: &mut egui::Ui) {
        if let Some(id) = self.selected_instance.clone() {
            self.instance_detail(ui, &id);
            return;
        }
        ui.horizontal(|ui| {
            ui.heading("实例");
            if ui.button("新建实例").clicked() {
                self.open_create_dialog();
            }
            ui.separator();
            ui.label("搜索：");
            ui.text_edit_singleline(&mut self.instance_search);
        });
        ui.separator();
        if self.instances.is_empty() {
            ui.label("暂无实例，点击“新建实例”开始。");
        }
        let search = self.instance_search.to_lowercase();
        let instances: Vec<Instance> = self
            .instances
            .iter()
            .filter(|instance| instance.name.to_lowercase().contains(&search))
            .cloned()
            .collect();
        let mut open_detail = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for instance in instances {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong(&instance.name);
                        ui.label(format!(
                            "{} · {}{}",
                            instance.game_version,
                            loader_label(&instance.loader),
                            instance
                                .loader_version
                                .as_deref()
                                .map(|version| format!(" {version}"))
                                .unwrap_or_default()
                        ));
                        ui.label(status_label(
                            &instance.install_status,
                            self.games.get(&instance.id).map(String::as_str),
                        ));
                        self.instance_buttons(ui, &instance.id, &instance.install_status);
                        if ui.button("详情").clicked() {
                            open_detail = Some(instance.id.clone());
                        }
                        if ui.button("复制").clicked() {
                            self.call(
                                "instance.copy",
                                json!({"id": instance.id, "name": format!("{} 副本", instance.name)}),
                                Value::Null,
                            );
                        }
                        if ui.button("删除").clicked() {
                            self.confirm_delete = Some(instance.id.clone());
                        }
                    });
                });
                ui.add_space(4.0);
            }
        });
        if let Some(id) = open_detail {
            self.selected_instance = Some(id);
            self.detail_tab = DetailTab::Overview;
        }
        self.create_dialog(ui.ctx());
        self.delete_dialog(ui.ctx());
    }

    fn delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.confirm_delete.clone() else {
            return;
        };
        let name = self.instance_name(&id);
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new("删除实例")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("确定删除实例“{name}”吗？"));
                ui.label("将删除该实例的 Mod、配置、存档、截图和日志，共享缓存不受影响。此操作不可撤销。");
                ui.horizontal(|ui| {
                    if ui.button("取消").clicked() {
                        cancel = true;
                    }
                    if ui.button("确认删除").clicked() {
                        confirm = true;
                    }
                });
            });
        if confirm {
            self.call("instance.delete", json!({"id": id}), Value::Null);
        }
        if confirm || cancel {
            self.confirm_delete = None;
        }
    }

    fn create_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.create.clone() else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;
        let mut loader_query = None;
        egui::Window::new("新建实例")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("名称：");
                    ui.text_edit_singleline(&mut dialog.name);
                });
                let previous_version = dialog.game_version.clone();
                ui.horizontal(|ui| {
                    ui.label("游戏版本：");
                    egui::ComboBox::from_id_salt("game_version")
                        .selected_text(if dialog.game_version.is_empty() {
                            "请选择"
                        } else {
                            &dialog.game_version
                        })
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for version in &self.versions {
                                ui.selectable_value(
                                    &mut dialog.game_version,
                                    version.id.clone(),
                                    format!("{}（{}）", version.id, version.release_type),
                                );
                            }
                        });
                    if ui
                        .checkbox(&mut dialog.include_snapshots, "包含快照")
                        .changed()
                    {
                        self.call(
                            "version.list",
                            json!({"includeSnapshots": dialog.include_snapshots}),
                            Value::Null,
                        );
                    }
                });
                if previous_version != dialog.game_version && dialog.loader != "vanilla" {
                    loader_query = Some((dialog.game_version.clone(), dialog.loader.clone()));
                }
                let previous_loader = dialog.loader.clone();
                ui.horizontal(|ui| {
                    ui.label("加载器：");
                    for (value, label) in [
                        ("vanilla", "Vanilla"),
                        ("fabric", "Fabric"),
                        ("forge", "Forge"),
                        ("neo_forge", "NeoForge"),
                    ] {
                        ui.selectable_value(&mut dialog.loader, value.to_owned(), label);
                    }
                });
                if previous_loader != dialog.loader {
                    dialog.loader_version.clear();
                    self.loader_versions.clear();
                    if dialog.loader != "vanilla" && !dialog.game_version.is_empty() {
                        loader_query = Some((dialog.game_version.clone(), dialog.loader.clone()));
                    }
                }
                if dialog.loader != "vanilla" {
                    ui.horizontal(|ui| {
                        ui.label("加载器版本：");
                        egui::ComboBox::from_id_salt("loader_version")
                            .selected_text(if dialog.loader_version.is_empty() {
                                "请选择"
                            } else {
                                &dialog.loader_version
                            })
                            .width(220.0)
                            .show_ui(ui, |ui| {
                                for version in &self.loader_versions {
                                    ui.selectable_value(
                                        &mut dialog.loader_version,
                                        version.version.clone(),
                                        format!(
                                            "{}{}",
                                            version.version,
                                            if version.stable { "（稳定）" } else { "" }
                                        ),
                                    );
                                }
                            });
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("取消").clicked() {
                        cancel = true;
                    }
                    let valid = !dialog.name.trim().is_empty()
                        && !dialog.game_version.is_empty()
                        && (dialog.loader == "vanilla" || !dialog.loader_version.is_empty());
                    if ui.add_enabled(valid, egui::Button::new("创建")).clicked() {
                        submit = true;
                    }
                });
            });
        if let Some((game_version, loader)) = loader_query {
            self.loader_versions.clear();
            self.call(
                "loader.list",
                json!({"gameVersion": game_version, "loader": loader}),
                Value::Null,
            );
        }
        if submit {
            self.call(
                "instance.create",
                json!({
                    "name": dialog.name.trim(),
                    "gameVersion": dialog.game_version,
                    "loader": dialog.loader,
                    "loaderVersion": if dialog.loader == "vanilla" { Value::Null } else { json!(dialog.loader_version) },
                    "javaPath": Value::Null,
                    "minMemoryMb": 512,
                    "maxMemoryMb": 4096,
                    "jvmArgs": [],
                    "gameArgs": [],
                    "accountId": Value::Null,
                }),
                Value::Null,
            );
        }
        if cancel {
            self.create = None;
        } else {
            self.create = Some(dialog);
        }
    }

    fn instance_detail(&mut self, ui: &mut egui::Ui, id: &str) {
        let Some(instance) = self.instance(id).cloned() else {
            ui.label("实例不存在或已被删除。");
            if ui.button("返回").clicked() {
                self.selected_instance = None;
            }
            return;
        };
        ui.horizontal(|ui| {
            if ui.button("← 返回列表").clicked() {
                self.selected_instance = None;
                return;
            }
            ui.heading(&instance.name);
            ui.label(format!(
                "{} · {}",
                instance.game_version,
                loader_label(&instance.loader)
            ));
            self.instance_buttons(ui, id, &instance.install_status);
        });
        ui.horizontal(|ui| {
            for (tab, label) in [
                (DetailTab::Overview, "概览"),
                (DetailTab::Mods, "Mod 管理"),
                (DetailTab::Logs, "日志"),
                (DetailTab::Launch, "启动设置"),
            ] {
                if ui.selectable_label(self.detail_tab == tab, label).clicked() {
                    self.detail_tab = tab;
                    match tab {
                        DetailTab::Mods => self.refresh_mods(id),
                        DetailTab::Logs => {
                            self.call(
                                "game.logs",
                                json!({"id": id, "tail": 500}),
                                json!({"instanceId": id}),
                            );
                        }
                        DetailTab::Launch => self.draft_instance = Some(instance.clone()),
                        DetailTab::Overview => {}
                    }
                }
            }
        });
        ui.separator();
        match self.detail_tab {
            DetailTab::Overview => self.instance_overview(ui, &instance),
            DetailTab::Mods => self.instance_mods(ui, id),
            DetailTab::Logs => self.instance_logs(ui, id),
            DetailTab::Launch => self.instance_launch_settings(ui, id),
        }
    }

    fn instance_overview(&mut self, ui: &mut egui::Ui, instance: &Instance) {
        ui.label(format!(
            "状态：{}",
            status_label(
                &instance.install_status,
                self.games.get(&instance.id).map(String::as_str)
            )
        ));
        if let Some(description) = &instance.description {
            ui.label(description);
        }
        if let Some(last) = &instance.last_launched_at {
            ui.label(format!("最近启动：{last}"));
        }
        if let Some(code) = self.exit_codes.get(&instance.id) {
            ui.label(format!("上次退出码：{code}"));
        }
        ui.add_space(8.0);
        if ui.button("启动前检查").clicked() {
            self.call(
                "game.preflight",
                json!({"id": instance.id}),
                json!({"instanceId": instance.id}),
            );
        }
        if let Some(result) = self.preflight.get(&instance.id).cloned() {
            let valid = result
                .get("valid")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if valid {
                ui.label("检查通过，可以启动。");
                if let Some(java) = result.get("java").and_then(|java| java.get("path")) {
                    ui.small(format!("Java：{}", java.as_str().unwrap_or_default()));
                }
            } else {
                ui.colored_label(egui::Color32::from_rgb(200, 60, 60), "启动前检查未通过：");
                if let Some(issues) = result.get("issues").and_then(Value::as_array) {
                    for issue in issues {
                        ui.label(format!("- {}", issue_text(issue)));
                    }
                }
            }
        }
    }

    fn instance_mods(&mut self, ui: &mut egui::Ui, id: &str) {
        ui.horizontal(|ui| {
            if ui.button("刷新").clicked() {
                self.refresh_mods(id);
            }
            if ui.button("检查更新").clicked() {
                self.call(
                    "mod.update",
                    json!({"instanceId": id}),
                    json!({"instanceId": id}),
                );
            }
            ui.label("手动放入实例 mods 目录的 jar 会显示为“未跟踪”。");
        });
        ui.separator();
        let mods = self.mods.get(id).cloned().unwrap_or_default();
        if mods.is_empty() {
            ui.label("该实例还没有 Mod。");
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for installed in mods {
                ui.horizontal(|ui| {
                    let mut enabled = installed.enabled;
                    if ui.checkbox(&mut enabled, "").changed() {
                        self.call(
                            "mod.set_enabled",
                            json!({"id": installed.id, "enabled": enabled}),
                            json!({"instanceId": id}),
                        );
                    }
                    ui.strong(&installed.file.project_name);
                    ui.label(&installed.file.version_name);
                    ui.label(if installed.managed {
                        installed.file.provider.as_str()
                    } else {
                        "未跟踪"
                    });
                    if ui.small_button("移除").clicked() {
                        self.call(
                            "mod.remove",
                            json!({"id": installed.id}),
                            json!({"instanceId": id}),
                        );
                    }
                });
            }
        });
    }

    fn instance_logs(&mut self, ui: &mut egui::Ui, id: &str) {
        if ui.button("重新加载日志").clicked() {
            self.call(
                "game.logs",
                json!({"id": id, "tail": 500}),
                json!({"instanceId": id}),
            );
        }
        ui.separator();
        let logs = self.logs.get(id).cloned().unwrap_or_default();
        if logs.is_empty() {
            ui.label("暂无日志。启动游戏后可在这里查看输出。");
        }
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in logs {
                    ui.add(egui::Label::new(egui::RichText::new(line).monospace().small()).wrap());
                }
            });
    }

    fn instance_launch_settings(&mut self, ui: &mut egui::Ui, id: &str) {
        let Some(mut draft) = self.draft_instance.clone() else {
            return;
        };
        if draft.id != id {
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Java 路径（留空自动选择）：");
            let mut java = draft.java_path.clone().unwrap_or_default();
            if ui.text_edit_singleline(&mut java).changed() {
                draft.java_path = if java.trim().is_empty() {
                    None
                } else {
                    Some(java.trim().to_owned())
                };
            }
        });
        ui.horizontal(|ui| {
            ui.label("最小内存 MB：");
            ui.add(egui::DragValue::new(&mut draft.min_memory_mb).range(256..=65536));
            ui.label("最大内存 MB：");
            ui.add(egui::DragValue::new(&mut draft.max_memory_mb).range(256..=65536));
        });
        ui.horizontal(|ui| {
            ui.label("额外 JVM 参数（空格分隔）：");
            let mut args = draft.jvm_args.join(" ");
            if ui.text_edit_singleline(&mut args).changed() {
                draft.jvm_args = args.split_whitespace().map(str::to_owned).collect();
            }
        });
        if ui.button("保存启动设置").clicked() {
            match serde_json::to_value(&draft) {
                Ok(value) => self.call("instance.update", value, Value::Null),
                Err(error) => self.notice = Some(format!("无法保存：{error}")),
            }
        }
        self.draft_instance = Some(draft);
    }
}

fn issue_text(issue: &Value) -> String {
    issue
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| issue.to_string())
}
