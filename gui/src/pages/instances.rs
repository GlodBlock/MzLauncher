use eframe::egui;
use serde_json::{json, Value};

use crate::app::{CreateDialog, DetailTab, MzApp, Page};
use crate::model::Instance;
use crate::pages::home::{loader_key, status_key};

const INSTANCE_ICON_SIZE: egui::Vec2 = egui::Vec2::new(48.0, 48.0);
const INSTANCE_ICON_GRASS_HEIGHT: f32 = 15.0;
const INSTANCE_ROW_CONTENT_HEIGHT: f32 = 64.0;

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
            let new_instance_label = self.tr("action.new_instance");
            let trailing_width = crate::layout::button_width(ui, new_instance_label.as_str());
            let search_width =
                (ui.available_width() - trailing_width - ui.spacing().item_spacing.x).max(0.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.instance_search)
                    .desired_width(search_width)
                    .hint_text(search_hint),
            );
            if ui.button(new_instance_label).clicked() {
                self.open_create_dialog();
            }
        });
        ui.add_space(crate::layout::CARD_LIST_GAP);
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
                ui.add_space(crate::layout::CARD_LIST_GAP);
            }
        });
        self.create_dialog(ui.ctx());
        self.delete_dialog(ui.ctx());
    }

    fn instance_row(&mut self, ui: &mut egui::Ui, instance: &Instance) {
        crate::card_frame(ui).show(ui, |ui| {
            let width = ui.available_width();
            ui.set_min_width(width);
            let running = matches!(
                self.games.get(&instance.id).map(String::as_str),
                Some("running" | "starting")
            );
            let installed = instance.install_status == "installed";
            let (primary_symbol, primary_tooltip, primary_method) = if running {
                ("■", "action.stop_game", "game.stop")
            } else if installed {
                ("▶", "action.launch_game", "game.launch")
            } else {
                ("⇩", "action.prepare_install", "instance.install")
            };
            ui.horizontal(|ui| {
                instance_icon(ui, &instance.name);
                let spacing = ui.spacing().item_spacing.x;
                let action_symbols = [primary_symbol, "⚙", "↗", "⋯"];
                let actions_width = action_symbols
                    .iter()
                    .map(|symbol| crate::layout::button_width(ui, *symbol))
                    .sum::<f32>()
                    + spacing * (action_symbols.len() - 1) as f32;
                let info_width = (ui.available_width() - actions_width - spacing).max(0.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(info_width, INSTANCE_ROW_CONTENT_HEIGHT),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&instance.name).size(17.0).strong(),
                            )
                            .truncate(),
                        );
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
                        ui.add(egui::Label::new(version).truncate());
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
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!("{status} · {last} · {played}")).small(),
                            )
                            .truncate(),
                        );
                    },
                );
                if ui
                    .button(primary_symbol)
                    .on_hover_text(self.tr(primary_tooltip))
                    .clicked()
                {
                    self.call(
                        primary_method,
                        json!({"id": instance.id}),
                        json!({"instanceId": instance.id}),
                    );
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
                egui::Grid::new("create_instance_form")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label(self.tr("create.name"));
                        ui.add(
                            egui::TextEdit::singleline(&mut dialog.name)
                                .desired_width(ui.available_width()),
                        );
                        ui.end_row();

                        let previous_version = dialog.game_version.clone();
                        ui.label(self.tr("create.game_version"));
                        let selected = if dialog.game_version.is_empty() {
                            self.tr("common.select")
                        } else {
                            dialog.game_version.clone()
                        };
                        egui::ComboBox::from_id_salt("game_version")
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                for version in &self.versions {
                                    ui.selectable_value(
                                        &mut dialog.game_version,
                                        version.id.clone(),
                                        format!("{}（{}）", version.id, version.release_type),
                                    );
                                }
                            });
                        ui.end_row();

                        ui.label("");
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
                        ui.end_row();

                        if previous_version != dialog.game_version && dialog.loader != "vanilla" {
                            loader_query =
                                Some((dialog.game_version.clone(), dialog.loader.clone()));
                        }
                        let previous_loader = dialog.loader.clone();
                        ui.label(self.tr("create.loader"));
                        ui.horizontal_wrapped(|ui| {
                            for (value, key) in [
                                ("vanilla", "loader.vanilla"),
                                ("fabric", "loader.fabric"),
                                ("forge", "loader.forge"),
                                ("neo_forge", "loader.neo_forge"),
                            ] {
                                ui.selectable_value(
                                    &mut dialog.loader,
                                    value.to_owned(),
                                    self.tr(key),
                                );
                            }
                        });
                        ui.end_row();
                        if previous_loader != dialog.loader {
                            dialog.loader_version.clear();
                            self.loader_versions.clear();
                            if dialog.loader != "vanilla" && !dialog.game_version.is_empty() {
                                loader_query =
                                    Some((dialog.game_version.clone(), dialog.loader.clone()));
                            }
                        }
                        if dialog.loader != "vanilla" {
                            ui.label(self.tr("create.loader_version"));
                            let selected = if dialog.loader_version.is_empty() {
                                self.tr("common.select")
                            } else {
                                dialog.loader_version.clone()
                            };
                            egui::ComboBox::from_id_salt("loader_version")
                                .selected_text(selected)
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
                            ui.end_row();
                        }
                    });
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
        ui.horizontal_wrapped(|ui| {
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
        ui.horizontal_wrapped(|ui| {
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
        ui.horizontal_wrapped(|ui| {
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
                ui.horizontal_wrapped(|ui| {
                    let mut enabled = installed.enabled;
                    if ui.checkbox(&mut enabled, "").changed() {
                        self.call(
                            "mod.set_enabled",
                            json!({"id": installed.id, "enabled": enabled}),
                            json!({"instanceId": id}),
                        );
                    }
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&installed.file.project_name).strong(),
                        )
                        .wrap(),
                    );
                    ui.add(egui::Label::new(&installed.file.version_name).wrap());
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
        egui::Grid::new("instance_launch_settings")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(self.tr("launch.java_path"));
                let mut java = draft.java_path.clone().unwrap_or_default();
                if ui
                    .add(egui::TextEdit::singleline(&mut java).desired_width(ui.available_width()))
                    .changed()
                {
                    draft.java_path = if java.trim().is_empty() {
                        None
                    } else {
                        Some(java.trim().to_owned())
                    };
                }
                ui.end_row();

                ui.label(self.tr("launch.min_memory"));
                ui.add(egui::DragValue::new(&mut draft.min_memory_mb).range(256..=65536));
                ui.end_row();

                ui.label(self.tr("launch.max_memory"));
                ui.add(egui::DragValue::new(&mut draft.max_memory_mb).range(256..=65536));
                ui.end_row();

                ui.label(self.tr("launch.jvm_args"));
                let mut args = draft.jvm_args.join(" ");
                if ui
                    .add(egui::TextEdit::singleline(&mut args).desired_width(ui.available_width()))
                    .changed()
                {
                    draft.jvm_args = args.split_whitespace().map(str::to_owned).collect();
                }
                ui.end_row();
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
    let (rect, _) = ui.allocate_exact_size(INSTANCE_ICON_SIZE, egui::Sense::hover());
    let (grass, dirt) = instance_colors(name);
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(8),
        dirt,
        egui::Stroke::NONE,
        egui::StrokeKind::Inside,
    );
    ui.painter().rect(
        egui::Rect::from_min_max(
            rect.min,
            egui::pos2(rect.max.x, rect.min.y + INSTANCE_ICON_GRASS_HEIGHT),
        ),
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
