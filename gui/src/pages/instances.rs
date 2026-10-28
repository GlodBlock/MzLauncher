use eframe::egui;
use serde_json::{json, Value};

use crate::app::{CreateDialog, DetailTab, MzApp, Page};
use crate::model::Instance;
use crate::pages::home::{loader_key, status_key};

impl MzApp {
    pub fn open_create_dialog(&mut self) {
        self.page = Page::Instances;
        self.selected_instance = None;
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
            ui.heading(self.tr("instances.title"));
            let search_hint = self.tr("instances.search_hint");
            ui.add(
                egui::TextEdit::singleline(&mut self.instance_search)
                    .desired_width(260.0)
                    .hint_text(search_hint),
            );
            if ui.button(self.tr("action.new_instance")).clicked() {
                self.open_create_dialog();
            }
        });
        ui.add_space(8.0);
        let search = self.instance_search.to_lowercase();
        let instances: Vec<Instance> = self
            .instances
            .iter()
            .filter(|instance| instance.name.to_lowercase().contains(&search))
            .cloned()
            .collect();
        if instances.is_empty() {
            ui.label(self.tr("instances.empty"));
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for instance in instances {
                self.instance_row(ui, &instance);
                ui.add_space(8.0);
            }
        });
        self.create_dialog(ui.ctx());
        self.delete_dialog(ui.ctx());
    }

    fn instance_row(&mut self, ui: &mut egui::Ui, instance: &Instance) {
        crate::card_frame(ui).show(ui, |ui| {
            let width = ui.available_width();
            ui.set_min_width(width);
            ui.horizontal(|ui| {
                instance_icon(ui, &instance.name);
                let action_width = 190.0;
                let info_width = (width - 48.0 - action_width - 24.0).max(120.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(info_width, 64.0),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        let name = character_ellipsis(&instance.name, 28);
                        ui.add(egui::Label::new(egui::RichText::new(name).size(17.0).strong()));
                        let version = format!(
                            "{} · {}{}",
                            instance.game_version,
                            self.tr(loader_key(&instance.loader)),
                            instance
                                .loader_version
                                .as_deref()
                                .map(|version| format!(" {version}"))
                                .unwrap_or_default()
                        );
                        ui.add(egui::Label::new(version));
                        let status = self.tr(status_key(
                            &instance.install_status,
                            self.games.get(&instance.id).map(String::as_str),
                        ));
                        let last = self.trf(
                            "instances.last_launch",
                            &[("time", &format_last_launch(instance.last_launched_at.as_deref()))],
                        );
                        let played = self.trf(
                            "instances.play_time",
                            &[("time", &format_play_time(instance.total_play_time_ms))],
                        );
                        let details = character_ellipsis(&format!("{status} · {last} · {played}"), 54);
                        ui.small(details);
                    },
                );
                let running = matches!(
                    self.games.get(&instance.id).map(String::as_str),
                    Some("running" | "starting")
                );
                if running {
                    if ui
                        .button("■")
                        .on_hover_text(self.tr("action.stop_game"))
                        .clicked()
                    {
                        self.call(
                            "game.stop",
                            json!({"id": instance.id}),
                            json!({"instanceId": instance.id}),
                        );
                    }
                } else {
                    let installed = instance.install_status == "installed";
                    let (symbol, tooltip) = if installed {
                        ("▶", "action.launch_game")
                    } else {
                        ("⇩", "action.prepare_install")
                    };
                    if ui.button(symbol).on_hover_text(self.tr(tooltip)).clicked() {
                        let method = if installed { "game.launch" } else { "instance.install" };
                        self.call(
                            method,
                            json!({"id": instance.id}),
                            json!({"instanceId": instance.id}),
                        );
                    }
                }
                if ui
                    .button("⚙")
                    .on_hover_text(self.tr("action.details"))
                    .clicked()
                {
                    self.selected_instance = Some(instance.id.clone());
                    self.detail_tab = DetailTab::Overview;
                }
                let can_open = self.core_instance_dir.is_some();
                if ui
                    .add_enabled(can_open, egui::Button::new("↗"))
                    .on_hover_text(self.tr("action.open_directory"))
                    .clicked()
                {
                    if let Some(directory) = &self.core_instance_dir {
                        let _ = open::that(std::path::PathBuf::from(directory).join(&instance.id));
                    }
                }
                ui.menu_button("⋯", |ui| {
                    if ui.button(self.tr("action.repair")).clicked() {
                        self.call(
                            "instance.install",
                            json!({"id": instance.id}),
                            json!({"instanceId": instance.id}),
                        );
                        ui.close_menu();
                    }
                    if ui.button(self.tr("action.copy")).clicked() {
                        self.call(
                            "instance.copy",
                            json!({"id": instance.id, "name": self.trf("instances.copy_name", &[("name", &instance.name)])}),
                            Value::Null,
                        );
                        ui.close_menu();
                    }
                    if ui.button(self.tr("action.delete_instance")).clicked() {
                        self.confirm_delete = Some(instance.id.clone());
                        ui.close_menu();
                    }
                });
            });
        });
    }

    fn delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.confirm_delete.clone() else {
            return;
        };
        let name = self.instance_name(&id);
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new(self.tr("delete.title"))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(self.trf("delete.confirm", &[("name", &name)]));
                ui.label(self.tr("delete.warning"));
                ui.horizontal(|ui| {
                    if ui.button(self.tr("action.cancel")).clicked() {
                        cancel = true;
                    }
                    if ui.button(self.tr("action.confirm_delete")).clicked() {
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
        egui::Window::new(self.tr("create.title"))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(self.tr("create.name"));
                    ui.text_edit_singleline(&mut dialog.name);
                });
                let previous_version = dialog.game_version.clone();
                ui.horizontal(|ui| {
                    ui.label(self.tr("create.game_version"));
                    let selected = if dialog.game_version.is_empty() {
                        self.tr("common.select")
                    } else {
                        dialog.game_version.clone()
                    };
                    egui::ComboBox::from_id_salt("game_version")
                        .selected_text(selected)
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
                        .checkbox(
                            &mut dialog.include_snapshots,
                            self.tr("create.include_snapshots"),
                        )
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
                    ui.label(self.tr("create.loader"));
                    for (value, key) in [
                        ("vanilla", "loader.vanilla"),
                        ("fabric", "loader.fabric"),
                        ("forge", "loader.forge"),
                        ("neo_forge", "loader.neo_forge"),
                    ] {
                        ui.selectable_value(&mut dialog.loader, value.to_owned(), self.tr(key));
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
                        ui.label(self.tr("create.loader_version"));
                        let selected = if dialog.loader_version.is_empty() {
                            self.tr("common.select")
                        } else {
                            dialog.loader_version.clone()
                        };
                        egui::ComboBox::from_id_salt("loader_version")
                            .selected_text(selected)
                            .width(220.0)
                            .show_ui(ui, |ui| {
                                for version in &self.loader_versions {
                                    ui.selectable_value(
                                        &mut dialog.loader_version,
                                        version.version.clone(),
                                        format!(
                                            "{}{}",
                                            version.version,
                                            if version.stable {
                                                self.tr("create.stable_suffix")
                                            } else {
                                                String::new()
                                            }
                                        ),
                                    );
                                }
                            });
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(self.tr("action.cancel")).clicked() {
                        cancel = true;
                    }
                    let valid = !dialog.name.trim().is_empty()
                        && !dialog.game_version.is_empty()
                        && (dialog.loader == "vanilla" || !dialog.loader_version.is_empty());
                    if ui
                        .add_enabled(valid, egui::Button::new(self.tr("action.create")))
                        .clicked()
                    {
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
                    "minMemoryMb": Value::Null,
                    "maxMemoryMb": Value::Null,
                    "jvmArgs": Value::Null,
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
            ui.label(self.tr("instances.missing"));
            if ui.button(self.tr("action.back")).clicked() {
                self.selected_instance = None;
            }
            return;
        };
        ui.horizontal(|ui| {
            if ui.button(self.tr("action.back_to_list")).clicked() {
                self.selected_instance = None;
                return;
            }
            ui.heading(&instance.name);
            ui.label(format!(
                "{} · {}",
                instance.game_version,
                self.tr(loader_key(&instance.loader))
            ));
            self.instance_buttons(ui, id, &instance.install_status);
        });
        ui.horizontal(|ui| {
            for (tab, key) in [
                (DetailTab::Overview, "tab.overview"),
                (DetailTab::Mods, "tab.mods"),
                (DetailTab::Logs, "tab.logs"),
                (DetailTab::Launch, "tab.launch"),
            ] {
                if ui
                    .selectable_label(self.detail_tab == tab, self.tr(key))
                    .clicked()
                {
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
        ui.label(self.trf(
            "overview.status",
            &[(
                "status",
                &self.tr(status_key(
                    &instance.install_status,
                    self.games.get(&instance.id).map(String::as_str),
                )),
            )],
        ));
        if let Some(description) = &instance.description {
            ui.label(description);
        }
        if let Some(last) = &instance.last_launched_at {
            ui.label(self.trf("overview.last_launch", &[("time", last)]));
        }
        ui.label(self.trf(
            "overview.play_time",
            &[("time", &format_play_time(instance.total_play_time_ms))],
        ));
        if let Some(code) = self.exit_codes.get(&instance.id) {
            ui.label(self.trf("overview.exit_code", &[("code", &code.to_string())]));
        }
        ui.add_space(8.0);
        if ui.button(self.tr("overview.preflight")).clicked() {
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
                ui.label(self.tr("overview.preflight_ok"));
                if let Some(java) = result.get("java").and_then(|java| java.get("path")) {
                    ui.small(self.trf(
                        "overview.preflight_java",
                        &[("path", java.as_str().unwrap_or_default())],
                    ));
                }
            } else {
                ui.colored_label(
                    egui::Color32::from_rgb(200, 60, 60),
                    self.tr("overview.preflight_failed"),
                );
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
            if ui.button(self.tr("action.refresh")).clicked() {
                self.refresh_mods(id);
            }
            if ui.button(self.tr("mods.check_update")).clicked() {
                self.call(
                    "mod.update",
                    json!({"instanceId": id}),
                    json!({"instanceId": id}),
                );
            }
            ui.label(self.tr("mods.manual_hint"));
        });
        ui.separator();
        let mods = self.mods.get(id).cloned().unwrap_or_default();
        if mods.is_empty() {
            ui.label(self.tr("mods.empty"));
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
                        installed.file.provider.clone()
                    } else {
                        self.tr("mods.untracked")
                    });
                    if ui.small_button(self.tr("action.remove")).clicked() {
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
        if ui.button(self.tr("logs.reload")).clicked() {
            self.call(
                "game.logs",
                json!({"id": id, "tail": 500}),
                json!({"instanceId": id}),
            );
        }
        ui.separator();
        let logs = self.logs.get(id).cloned().unwrap_or_default();
        if logs.is_empty() {
            ui.label(self.tr("logs.empty"));
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
            ui.label(self.tr("launch.java_path"));
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
            ui.label(self.tr("launch.min_memory"));
            ui.add(egui::DragValue::new(&mut draft.min_memory_mb).range(256..=65536));
            ui.label(self.tr("launch.max_memory"));
            ui.add(egui::DragValue::new(&mut draft.max_memory_mb).range(256..=65536));
        });
        ui.horizontal(|ui| {
            ui.label(self.tr("launch.jvm_args"));
            let mut args = draft.jvm_args.join(" ");
            if ui.text_edit_singleline(&mut args).changed() {
                draft.jvm_args = args.split_whitespace().map(str::to_owned).collect();
            }
        });
        if ui.button(self.tr("launch.save")).clicked() {
            match serde_json::to_value(&draft) {
                Ok(value) => self.call("instance.update", value, Value::Null),
                Err(error) => {
                    self.notice =
                        Some(self.trf("launch.save_error", &[("error", &error.to_string())]))
                }
            }
        }
        self.draft_instance = Some(draft);
    }
}

fn instance_icon(ui: &mut egui::Ui, name: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
    let (grass, dirt) = instance_colors(name);
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(8),
        dirt,
        egui::Stroke::NONE,
        egui::StrokeKind::Inside,
    );
    ui.painter().rect(
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + 15.0)),
        egui::CornerRadius::same(6),
        grass,
        egui::Stroke::NONE,
        egui::StrokeKind::Inside,
    );
}

fn format_last_launch(value: Option<&str>) -> String {
    let Some(value) = value else {
        return "—".into();
    };
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| value.to_owned())
}

fn format_play_time(milliseconds: u64) -> String {
    let seconds = milliseconds / 1000;
    let days = seconds / 86400;
    let hours = seconds % 86400 / 3600;
    let minutes = seconds % 3600 / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{seconds}s")
    }
}

fn character_ellipsis(text: &str, max_units: usize) -> String {
    let mut units = 0;
    let mut output = String::new();
    for character in text.chars() {
        let width = if character.is_ascii() { 1 } else { 2 };
        if units + width > max_units {
            output.push('…');
            return output;
        }
        units += width;
        output.push(character);
    }
    output
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
