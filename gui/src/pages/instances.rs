use eframe::egui;
use serde_json::{json, Value};

use crate::app::{DetailTab, InstallingModpack, MzApp, NewInstanceState, NewInstanceTab, Page};
use crate::model::{Instance, Job};
use crate::pages::home::{job_status_key, loader_key, status_key};

const INSTANCE_ICON_SIZE: egui::Vec2 = egui::Vec2::new(48.0, 48.0);
const INSTANCE_ICON_GRASS_HEIGHT: f32 = 15.0;
const INSTANCE_ROW_CONTENT_HEIGHT: f32 = 64.0;
const NEW_INSTANCE_BAR_HEIGHT: f32 = 48.0;
const NEW_INSTANCE_ICON_SIZE: egui::Vec2 = egui::Vec2::new(18.0, 18.0);
const NEW_INSTANCE_ICON_TEXT_GAP: f32 = 8.0;
const INSTALL_ACTION_SIZE: egui::Vec2 = crate::layout::TOOLBAR_CONTROL_SIZE;

impl MzApp {
    pub fn open_create_dialog(&mut self) {
        self.page = Page::Instances;
        self.create = Some(NewInstanceState {
            name: String::new(),
            game_version: String::new(),
            loader: "vanilla".into(),
            loader_version: String::new(),
            include_snapshots: false,
            tab: NewInstanceTab::CurseForge,
            creating: false,
        });
        self.modpack.path.clear();
        self.modpack.name.clear();
        self.modpack.analysis = None;
        self.modpack.analyzing = false;
        self.modpack.importing = false;
        self.versions.clear();
        self.loader_versions.clear();
        self.call(
            "version.list",
            json!({"includeSnapshots": false}),
            json!({"includeSnapshots": false}),
        );
        self.search_modpacks();
    }

    pub fn instances_page(&mut self, ui: &mut egui::Ui) {
        if let Some(id) = self.selected_instance.clone() {
            self.instance_detail(ui, &id);
            return;
        }
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), crate::layout::TOOLBAR_CONTROL_SIZE.y),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.heading(self.tr("instances.title"));
                let spacing = ui.spacing().item_spacing.x;
                let search_width = (ui.available_width()
                    - INSTALL_ACTION_SIZE.x
                    - spacing
                    - crate::layout::CONTENT_TRAILING_RESERVE)
                    .max(0.0);
                let search_hint = self.tr("instances.search_hint");
                crate::widgets::search_field(
                    ui,
                    &mut self.instance_search,
                    search_hint,
                    search_width,
                );
                crate::widgets::search_button(ui, &self.tr("create.search"), true);
            },
        );
        ui.add_space(crate::layout::CARD_LIST_GAP);
        let search = self.instance_search.to_lowercase();
        let installing: Vec<InstallingModpack> = self
            .installing_modpacks
            .iter()
            .filter(|install| {
                install.project.name.to_lowercase().contains(&search)
                    && !self.installation_has_instance(install)
            })
            .cloned()
            .collect();
        let instances: Vec<Instance> = self
            .instances
            .iter()
            .filter(|instance| instance.name.to_lowercase().contains(&search))
            .cloned()
            .collect();
        if new_instance_button(ui, &self.tr("action.new_instance")).clicked() {
            self.open_create_dialog();
        }
        ui.add_space(crate::layout::CARD_LIST_GAP);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for install in installing {
                let job = install
                    .job_id
                    .as_ref()
                    .and_then(|id| self.jobs.iter().find(|job| &job.id == id))
                    .cloned();
                let version = self.modpack_install_version(&install);
                self.installing_row(
                    ui,
                    &install.project.name,
                    &version,
                    Some(&install.project),
                    job.as_ref(),
                );
                ui.add_space(crate::layout::CARD_LIST_GAP);
            }
            for instance in instances {
                self.instance_row(ui, &instance);
                ui.add_space(crate::layout::CARD_LIST_GAP);
            }
        });
        self.delete_dialog(ui.ctx());
        self.install_details_dialog(ui.ctx());
    }

    fn installation_has_instance(&self, install: &InstallingModpack) -> bool {
        install
            .job_id
            .as_ref()
            .and_then(|id| self.jobs.iter().find(|job| &job.id == id))
            .and_then(Job::instance_id)
            .is_some_and(|id| self.instances.iter().any(|instance| instance.id == id))
    }

    fn displayed_installation_job(&self, instance_id: &str) -> Option<Job> {
        self.jobs
            .iter()
            .find(|job| {
                matches!(job.kind.as_str(), "import" | "install")
                    && (matches!(job.status.as_str(), "queued" | "running" | "cancelling")
                        || (job.kind == "import" && job.status == "interrupted"))
                    && job.instance_id() == Some(instance_id)
            })
            .cloned()
    }

    fn instance_version(&self, instance: &Instance) -> String {
        format!(
            "{} · {}{}",
            instance.game_version,
            self.tr(loader_key(&instance.loader)),
            instance
                .loader_version
                .as_deref()
                .map(|version| format!(" {version}"))
                .unwrap_or_default()
        )
    }

    fn modpack_install_version(&self, install: &InstallingModpack) -> String {
        let game_version = if install.game_version.is_empty() {
            if install.project.game_versions.is_empty() {
                "—".into()
            } else {
                install.project.game_versions.join(", ")
            }
        } else {
            install.game_version.clone()
        };
        let loader = if install.loader.is_empty() {
            install
                .project
                .loaders
                .first()
                .map(String::as_str)
                .unwrap_or("vanilla")
        } else {
            &install.loader
        };
        format!("{} · {}", game_version, self.tr(loader_key(loader)))
    }

    fn installing_row(
        &mut self,
        ui: &mut egui::Ui,
        name: &str,
        version: &str,
        project: Option<&crate::model::ModProject>,
        job: Option<&Job>,
    ) {
        let mut retry = None;
        let mut details = None;
        let mut delete = None;
        crate::card_frame(ui).show(ui, |ui| {
            let width = ui.available_width();
            ui.set_min_width(width);
            ui.allocate_ui_with_layout(
                egui::vec2(width, INSTANCE_ROW_CONTENT_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let texture = project.and_then(|project| {
                        self.modpack_icons
                            .texture(ui.ctx(), &crate::app::modpack_icon_key(project))
                    });
                    if let Some(texture) = texture {
                        let (rect, _) =
                            ui.allocate_exact_size(INSTANCE_ICON_SIZE, egui::Sense::hover());
                        egui::Image::new((texture.id(), INSTANCE_ICON_SIZE))
                            .corner_radius(egui::CornerRadius::same(8))
                            .paint_at(ui, rect);
                    } else {
                        instance_icon(ui, name);
                    }
                    let spacing = ui.spacing().item_spacing.x;
                    let info_width = (ui.available_width()
                        - INSTALL_ACTION_SIZE.x * 3.0
                        - spacing * 3.0
                        - crate::layout::CONTENT_TRAILING_RESERVE)
                        .max(0.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(info_width, INSTANCE_ROW_CONTENT_HEIGHT),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            ui.add(
                                egui::Label::new(egui::RichText::new(name).size(17.0).strong())
                                    .truncate(),
                            );
                            ui.add(egui::Label::new(version).truncate());
                            let status = job.map_or_else(
                                || self.tr("job.status.queued"),
                                |job| {
                                    if job.status == "running" {
                                        self.tr("status.installing")
                                    } else {
                                        self.tr(job_status_key(&job.status))
                                    }
                                },
                            );
                            let fraction = job.and_then(|job| {
                                job.progress.total.filter(|total| *total > 0).map(|total| {
                                    (job.progress.completed as f32 / total as f32).clamp(0.0, 1.0)
                                })
                            });
                            let text = fraction.map_or(status.clone(), |fraction| {
                                format!("{status} · {:.0}%", fraction * 100.0)
                            });
                            crate::widgets::themed_progress_bar(
                                ui,
                                fraction.unwrap_or(0.0),
                                info_width,
                                text,
                            );
                        },
                    );
                    if job.is_none_or(|job| {
                        matches!(job.status.as_str(), "queued" | "running" | "cancelling")
                    }) {
                        ui.allocate_ui_with_layout(
                            INSTALL_ACTION_SIZE,
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.spinner();
                            },
                        );
                    } else if let Some(job) =
                        job.filter(|job| matches!(job.status.as_str(), "failed" | "interrupted"))
                    {
                        if crate::widgets::IconButton::new(
                            "retry-install",
                            include_bytes!("../../assets/rotate-right-light.png"),
                            include_bytes!("../../assets/rotate-right-dark.png"),
                        )
                        .show(ui, &self.tr("action.retry_install"))
                        .clicked()
                        {
                            retry = Some(job.id.clone());
                        }
                    }
                    if let Some(job) = job {
                        if install_details_button(ui, &self.tr("action.details")).clicked() {
                            details = Some(job.id.clone());
                        }
                        if delete_instance_button(ui, &self.tr("action.delete_instance")).clicked()
                        {
                            delete = Some((job.id.clone(), job.instance_id().map(str::to_owned)));
                        }
                    }
                },
            );
        });
        if let Some(job_id) = retry {
            self.retry_modpack_install(&job_id);
        }
        if let Some(job_id) = details {
            self.install_details_job = Some(job_id);
        }
        if let Some((job_id, instance_id)) = delete {
            self.confirm_delete = Some(crate::app::DeleteConfirmation {
                instance_id,
                job_id: Some(job_id),
                name: name.to_owned(),
            });
        }
    }

    fn instance_row(&mut self, ui: &mut egui::Ui, instance: &Instance) {
        let version = self.instance_version(instance);
        if let Some(job) = self.displayed_installation_job(&instance.id) {
            let project = self
                .installing_modpacks
                .iter()
                .find(|install| install.job_id.as_deref() == Some(job.id.as_str()))
                .map(|install| install.project.clone());
            self.installing_row(ui, &instance.name, &version, project.as_ref(), Some(&job));
            return;
        }
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
                    + INSTALL_ACTION_SIZE.x
                    + spacing * action_symbols.len() as f32;
                let info_width = (ui.available_width() - actions_width - spacing - crate::layout::CONTENT_TRAILING_RESERVE)
                        .max(0.0);
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
                });
                if delete_instance_button(ui, &self.tr("action.delete_instance")).clicked() {
                    self.confirm_delete = Some(crate::app::DeleteConfirmation {
                        instance_id: Some(instance.id.clone()),
                        job_id: None,
                        name: instance.name.clone(),
                    });
                }
            });
        });
    }

    fn delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(confirmation) = self.confirm_delete.clone() else {
            return;
        };
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new(self.tr("delete.title"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(self.trf("delete.confirm", &[("name", &confirmation.name)]));
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
            if let Some(id) = confirmation.instance_id {
                self.call("instance.delete", json!({"id": id}), Value::Null);
            } else if let Some(id) = confirmation.job_id {
                self.call("job.cancel", json!({"id": id}), Value::Null);
            }
        }
        if confirm || cancel {
            self.confirm_delete = None;
        }
    }

    fn install_details_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.install_details_job.clone() else {
            return;
        };
        let Some(job) = self.jobs.iter().find(|job| job.id == id).cloned() else {
            self.install_details_job = None;
            return;
        };
        let current = job
            .progress
            .current_item
            .unwrap_or_else(|| self.tr("install_details.empty"));
        let mut close = false;
        egui::Window::new(self.tr("install_details.title"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .default_width(480.0)
            .show(ctx, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(egui::RichText::new(self.tr("install_details.current")).strong());
                ui.add(egui::Label::new(current).wrap());
                ui.separator();
                ui.label(egui::RichText::new(self.tr("install_details.pending")).strong());
                if job.progress.pending_items.is_empty() {
                    ui.label(self.tr("install_details.empty"));
                } else {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(240.0)
                        .show(ui, |ui| {
                            for item in &job.progress.pending_items {
                                ui.add(egui::Label::new(item).wrap());
                            }
                        });
                }
                ui.separator();
                if ui.button(self.tr("install_details.close")).clicked() {
                    close = true;
                }
            });
        if close {
            self.install_details_job = None;
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

fn new_instance_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let image = crate::widgets::themed_image(
        ui,
        "new-instance-plus",
        include_bytes!("../../assets/plus-light.png"),
        include_bytes!("../../assets/plus-dark.png"),
        NEW_INSTANCE_ICON_SIZE,
    );
    ui.scope(|ui| {
        ui.spacing_mut().icon_spacing = NEW_INSTANCE_ICON_TEXT_GAP;
        ui.add(
            egui::Button::image_and_text(image, label)
                .min_size(egui::vec2(ui.available_width(), NEW_INSTANCE_BAR_HEIGHT)),
        )
    })
    .inner
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn install_details_button(ui: &mut egui::Ui, tooltip: &str) -> egui::Response {
    crate::widgets::IconButton::new(
        "install-details",
        include_bytes!("../../assets/info-light.png"),
        include_bytes!("../../assets/info-dark.png"),
    )
    .show(ui, tooltip)
}

fn delete_instance_button(ui: &mut egui::Ui, tooltip: &str) -> egui::Response {
    crate::widgets::IconButton::new(
        "delete-instance",
        include_bytes!("../../assets/trash-light.png"),
        include_bytes!("../../assets/trash-dark.png"),
    )
    .show(ui, tooltip)
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
