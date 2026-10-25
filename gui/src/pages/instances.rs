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
            if ui.button("导入整合包").clicked() {
                self.page = crate::app::Page::Modpack;
            }
            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut self.instance_search)
                    .desired_width(200.0)
                    .hint_text("搜索实例"),
            );
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
        if let Some(selected) = self.grid_selected.clone() {
            self.instance_action_panel(ui.ctx(), &selected);
        }
        let mut clicked = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for instance in instances {
                    let selected = self.grid_selected.as_deref() == Some(instance.id.as_str());
                    if instance_tile(ui, &instance, selected).clicked() {
                        clicked = Some(instance.id.clone());
                    }
                }
            });
        });
        if clicked.is_some() {
            self.grid_selected = clicked;
        }
        self.create_dialog(ui.ctx());
        self.delete_dialog(ui.ctx());
    }

    fn instance_action_panel(&mut self, ctx: &egui::Context, id: &str) {
        let Some(instance) = self.instance(id).cloned() else {
            self.grid_selected = None;
            return;
        };
        egui::SidePanel::right("instance_actions")
            .resizable(false)
            .exact_width(240.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.heading(&instance.name);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").clicked() {
                            self.grid_selected = None;
                        }
                    });
                });
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
                ui.add_space(12.0);
                let width = ui.available_width();
                let running = matches!(
                    self.games.get(id).map(String::as_str),
                    Some("running" | "starting")
                );
                if running {
                    if ui
                        .add_sized([width, 32.0], egui::Button::new("停止游戏"))
                        .clicked()
                    {
                        self.call("game.stop", json!({"id": id}), json!({"instanceId": id}));
                    }
                } else if ui
                    .add_sized(
                        [width, 32.0],
                        egui::Button::new(if instance.install_status == "installed" {
                            "启动游戏"
                        } else {
                            "安装并准备启动"
                        }),
                    )
                    .clicked()
                {
                    if instance.install_status == "installed" {
                        self.call("game.launch", json!({"id": id}), json!({"instanceId": id}));
                    } else {
                        self.call(
                            "instance.install",
                            json!({"id": id}),
                            json!({"instanceId": id}),
                        );
                    }
                }
                if ui
                    .add_sized([width, 30.0], egui::Button::new("安装 / 修复"))
                    .clicked()
                {
                    self.call(
                        "instance.install",
                        json!({"id": id}),
                        json!({"instanceId": id}),
                    );
                }
                if ui
                    .add_sized([width, 30.0], egui::Button::new("打开详情"))
                    .clicked()
                {
                    self.selected_instance = Some(id.to_owned());
                    self.detail_tab = DetailTab::Overview;
                }
                if ui
                    .add_sized([width, 30.0], egui::Button::new("创建副本"))
                    .clicked()
                {
                    self.call(
                        "instance.copy",
                        json!({"id": id, "name": format!("{} 副本", instance.name)}),
                        Value::Null,
                    );
                }
                ui.add_space(8.0);
                if ui
                    .add_sized(
                        [width, 30.0],
                        egui::Button::new(
                            egui::RichText::new("删除实例")
                                .color(egui::Color32::from_rgb(190, 50, 50)),
                        ),
                    )
                    .clicked()
                {
                    self.confirm_delete = Some(id.to_owned());
                }
            });
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

fn instance_tile(ui: &mut egui::Ui, instance: &Instance, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(136.0, 132.0), egui::Sense::click());
    let accent = egui::Color32::from_rgb(59, 130, 246);
    let fill = if selected {
        egui::Color32::from_rgb(227, 238, 253)
    } else if response.hovered() {
        egui::Color32::from_rgb(240, 244, 250)
    } else {
        egui::Color32::WHITE
    };
    let stroke = if selected || response.hovered() {
        accent
    } else {
        egui::Color32::from_rgb(228, 230, 234)
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(8),
        fill,
        egui::Stroke::new(1.0_f32, stroke),
        egui::StrokeKind::Inside,
    );

    let icon = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 34.0),
        egui::vec2(48.0, 48.0),
    );
    let (grass, dirt) = instance_colors(&instance.name);
    ui.painter().rect(
        icon,
        egui::CornerRadius::same(8),
        dirt,
        egui::Stroke::NONE,
        egui::StrokeKind::Inside,
    );
    ui.painter().rect(
        egui::Rect::from_min_max(icon.min, egui::pos2(icon.max.x, icon.min.y + 15.0)),
        egui::CornerRadius::same(6),
        grass,
        egui::Stroke::NONE,
        egui::StrokeKind::Inside,
    );

    let mut name = instance.name.clone();
    if name.chars().count() > 14 {
        name = format!("{}…", name.chars().take(13).collect::<String>());
    }
    ui.painter().text(
        egui::pos2(rect.center().x, rect.top() + 64.0),
        egui::Align2::CENTER_TOP,
        name,
        egui::FontId::proportional(13.0),
        egui::Color32::from_rgb(35, 38, 43),
    );
    ui.painter().text(
        egui::pos2(rect.center().x, rect.top() + 86.0),
        egui::Align2::CENTER_TOP,
        format!(
            "{} · {}",
            instance.game_version,
            loader_label(&instance.loader)
        ),
        egui::FontId::proportional(11.0),
        egui::Color32::from_rgb(120, 126, 134),
    );
    ui.painter().text(
        egui::pos2(rect.center().x, rect.top() + 106.0),
        egui::Align2::CENTER_TOP,
        match instance.install_status.as_str() {
            "installed" => "可启动",
            "installing" => "安装中",
            _ => "未安装",
        },
        egui::FontId::proportional(11.0),
        if instance.install_status == "installed" {
            egui::Color32::from_rgb(34, 150, 84)
        } else {
            egui::Color32::from_rgb(150, 120, 40)
        },
    );
    response
}

fn instance_colors(name: &str) -> (egui::Color32, egui::Color32) {
    let hash = name.bytes().fold(0_u32, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(byte as u32)
    });
    let palette = [
        ((104, 168, 76), (134, 96, 67)),
        ((86, 142, 190), (110, 94, 78)),
        ((150, 110, 180), (122, 92, 70)),
        ((192, 146, 62), (128, 100, 66)),
        ((76, 164, 152), (118, 96, 74)),
        ((176, 96, 96), (126, 94, 68)),
    ];
    let (grass, dirt) = palette[(hash % palette.len() as u32) as usize];
    (
        egui::Color32::from_rgb(grass.0, grass.1, grass.2),
        egui::Color32::from_rgb(dirt.0, dirt.1, dirt.2),
    )
}

fn issue_text(issue: &Value) -> String {
    issue
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| issue.to_string())
}
