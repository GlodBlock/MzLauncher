use eframe::egui;
use serde_json::{json, Value};

use crate::app::{ConfigurationTab, InstanceModpackConfiguration, MzApp};
use crate::model::Instance;
use crate::pages::home::{job_stage_key, loader_key};

const SIDEBAR_WIDTH: f32 = 180.0;
const TAB_BUTTON_HEIGHT: f32 = 44.0;
const WINDOW_SIZE: [f32; 2] = [1040.0, 700.0];
const MIN_WINDOW_SIZE: [f32; 2] = [760.0, 520.0];

impl MzApp {
    pub fn open_instance_configuration(&mut self, id: &str) {
        let Some(instance) = self.instance(id).cloned() else {
            return;
        };
        self.selected_instance = Some(id.to_owned());
        self.configuration_tab = ConfigurationTab::Launch;
        self.instance_min_memory_input = instance.min_memory_mb.to_string();
        self.instance_max_memory_input = instance.max_memory_mb.to_string();
        self.instance_jvm_args_input = instance.jvm_args.join(" ");
        self.draft_instance = Some(instance);
        self.instance_modpack = InstanceModpackConfiguration::default();
        self.refresh_mods(id);
        self.call("modpack.get", json!({"id": id}), json!({"instanceId": id}));
    }

    pub fn close_instance_configuration(&mut self) {
        self.selected_instance = None;
        self.draft_instance = None;
        self.instance_min_memory_input.clear();
        self.instance_max_memory_input.clear();
        self.instance_jvm_args_input.clear();
        self.instance_versions.clear();
        self.instance_loader_versions.clear();
        self.instance_modpack = InstanceModpackConfiguration::default();
    }

    pub fn instance_configuration_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.selected_instance.clone() else {
            return;
        };
        let title = self.trf("configuration.title", &[("name", &self.instance_name(&id))]);
        let mut close = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of(("instance_configuration_window", &id)),
            egui::ViewportBuilder::default()
                .with_title(title)
                .with_inner_size(WINDOW_SIZE)
                .with_min_inner_size(MIN_WINDOW_SIZE),
            |ctx, _class| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.instance_configuration_page(ui, &id);
                });
                if ctx.input(|input| input.viewport().close_requested()) {
                    close = true;
                }
            },
        );
        if close {
            self.close_instance_configuration();
        }
    }

    fn instance_configuration_page(&mut self, ui: &mut egui::Ui, id: &str) {
        let tab = self.configuration_tab;
        let mut selected = None;
        egui::SidePanel::left("instance_configuration_tabs")
            .resizable(false)
            .exact_width(SIDEBAR_WIDTH)
            .show_inside(ui, |ui| {
                for (tab, key) in [
                    (ConfigurationTab::Launch, "configuration.launch"),
                    (ConfigurationTab::Runtime, "configuration.runtime"),
                    (ConfigurationTab::Modpack, "configuration.modpack"),
                ] {
                    if ui
                        .add_sized(
                            [ui.available_width(), TAB_BUTTON_HEIGHT],
                            egui::SelectableLabel::new(tab == self.configuration_tab, self.tr(key)),
                        )
                        .clicked()
                    {
                        selected = Some(tab);
                    }
                    ui.add_space(crate::layout::CARD_LIST_GAP);
                }
            });
        egui::CentralPanel::default().show_inside(ui, |ui| match tab {
            ConfigurationTab::Launch => self.instance_launch_configuration(ui, id),
            ConfigurationTab::Runtime => self.instance_runtime_configuration(ui, id),
            ConfigurationTab::Modpack => self.instance_modpack_configuration(ui, id),
        });
        if let Some(tab) = selected {
            self.select_instance_configuration_tab(id, tab);
        }
    }

    fn select_instance_configuration_tab(&mut self, id: &str, tab: ConfigurationTab) {
        if self.configuration_tab == tab {
            return;
        }
        self.configuration_tab = tab;
        match tab {
            ConfigurationTab::Launch => {}
            ConfigurationTab::Runtime => self.request_instance_runtime_versions(id),
            ConfigurationTab::Modpack => self.refresh_mods(id),
        }
    }

    fn request_instance_runtime_versions(&mut self, id: &str) {
        self.instance_versions.clear();
        self.call(
            "version.list",
            json!({"includeSnapshots": true}),
            json!({"configurationId": id, "includeSnapshots": true}),
        );
        self.request_instance_loader_versions(id);
    }

    fn request_instance_loader_versions(&mut self, id: &str) {
        let Some(draft) = &self.draft_instance else {
            return;
        };
        self.instance_loader_versions.clear();
        if draft.loader == "vanilla" {
            return;
        }
        self.call(
            "loader.list",
            json!({"gameVersion": draft.game_version, "loader": draft.loader}),
            json!({
                "configurationId": id,
                "gameVersion": draft.game_version,
                "loader": draft.loader,
            }),
        );
    }

    fn instance_launch_configuration(&mut self, ui: &mut egui::Ui, _id: &str) {
        let Some(mut draft) = self.draft_instance.clone() else {
            return;
        };
        let original = draft.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("instance_launch_configuration")
                .num_columns(2)
                .show(ui, |ui| {
                    let control_height = crate::layout::button_height(ui);
                    ui.spacing_mut().interact_size.y = control_height;
                    ui.label(self.tr("launch.java_path"));
                    let mut java = draft.java_path.clone().unwrap_or_default();
                    let browse_label = self.tr("modpack.browse");
                    ui.horizontal(|ui| {
                        let browse_width = crate::layout::button_width(ui, &browse_label);
                        let java_width = (ui.available_width()
                            - browse_width
                            - ui.spacing().item_spacing.x
                            - crate::layout::CONTENT_TRAILING_RESERVE)
                            .max(0.0);
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut java)
                                    .desired_width(java_width)
                                    .min_size(egui::vec2(java_width, control_height))
                                    .vertical_align(egui::Align::Center),
                            )
                            .changed()
                        {
                            draft.java_path = nonempty(java);
                        }
                        if ui.button(browse_label).clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                draft.java_path = nonempty(path.display().to_string());
                            }
                        }
                    });
                    ui.end_row();

                    ui.label(self.tr("launch.min_memory"));
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.instance_min_memory_input)
                                .desired_width(ui.available_width())
                                .min_size(egui::vec2(ui.available_width(), control_height))
                                .vertical_align(egui::Align::Center),
                        )
                        .changed()
                    {
                        self.instance_min_memory_input
                            .retain(|character| character.is_ascii_digit());
                        if let Some(value) = positive_memory(&self.instance_min_memory_input) {
                            draft.min_memory_mb = value;
                        }
                    }
                    if positive_memory(&self.instance_min_memory_input).is_none() {
                        ui.end_row();
                        ui.label("");
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            self.tr("launch.memory_positive"),
                        );
                    }
                    ui.end_row();

                    ui.label(self.tr("launch.max_memory"));
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.instance_max_memory_input)
                                .desired_width(ui.available_width())
                                .min_size(egui::vec2(ui.available_width(), control_height))
                                .vertical_align(egui::Align::Center),
                        )
                        .changed()
                    {
                        self.instance_max_memory_input
                            .retain(|character| character.is_ascii_digit());
                        if let Some(value) = positive_memory(&self.instance_max_memory_input) {
                            draft.max_memory_mb = value;
                        }
                    }
                    if positive_memory(&self.instance_max_memory_input).is_none() {
                        ui.end_row();
                        ui.label("");
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            self.tr("launch.memory_positive"),
                        );
                    }
                    ui.end_row();

                    let jvm_args_rows = 4;
                    let jvm_args_height =
                        jvm_args_rows as f32 * ui.text_style_height(&egui::TextStyle::Body) + 4.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), jvm_args_height),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            ui.label(self.tr("launch.jvm_args"));
                        },
                    );
                    if ui
                        .add(
                            egui::TextEdit::multiline(&mut self.instance_jvm_args_input)
                                .desired_width(ui.available_width())
                                .desired_rows(jvm_args_rows)
                                .min_size(egui::vec2(ui.available_width(), jvm_args_height))
                                .vertical_align(egui::Align::Min),
                        )
                        .changed()
                    {
                        draft.jvm_args = self
                            .instance_jvm_args_input
                            .split_whitespace()
                            .map(str::to_owned)
                            .collect();
                    }
                    ui.end_row();
                });
        });
        let min_memory = positive_memory(&self.instance_min_memory_input);
        let max_memory = positive_memory(&self.instance_max_memory_input);
        let memory_valid = min_memory.is_some() && max_memory.is_some();
        if let (Some(minimum), Some(maximum)) = (min_memory, max_memory) {
            draft.max_memory_mb = maximum;
            draft.min_memory_mb = minimum.min(maximum);
            if minimum > maximum {
                self.instance_min_memory_input = maximum.to_string();
            }
        }
        self.draft_instance = Some(draft.clone());
        if memory_valid
            && (draft.java_path != original.java_path
                || draft.min_memory_mb != original.min_memory_mb
                || draft.max_memory_mb != original.max_memory_mb
                || draft.jvm_args != original.jvm_args)
        {
            self.save_instance_configuration(draft);
        }
    }

    fn instance_runtime_configuration(&mut self, ui: &mut egui::Ui, id: &str) {
        let Some(mut draft) = self.draft_instance.clone() else {
            return;
        };
        let original = draft.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(self.tr("configuration.runtime_hint"));
            ui.add_space(crate::layout::SECTION_GAP);
            egui::Grid::new("instance_runtime_configuration")
                .num_columns(2)
                .show(ui, |ui| {
                    let control_height = crate::layout::button_height(ui);
                    ui.spacing_mut().interact_size.y = control_height;
                    ui.label(self.tr("configuration.lwjgl_version"));
                    let mut lwjgl = draft.lwjgl_version.clone().unwrap_or_default();
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut lwjgl)
                                .hint_text(self.tr("configuration.lwjgl_hint"))
                                .desired_width(ui.available_width())
                                .min_size(egui::vec2(ui.available_width(), control_height))
                                .vertical_align(egui::Align::Center),
                        )
                        .changed()
                    {
                        draft.lwjgl_version = nonempty(lwjgl);
                    }
                    ui.end_row();

                    ui.label(self.tr("create.game_version"));
                    egui::ComboBox::from_id_salt("instance_configuration_game_version")
                        .selected_text(&draft.game_version)
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for version in &self.instance_versions {
                                ui.selectable_value(
                                    &mut draft.game_version,
                                    version.id.clone(),
                                    &version.id,
                                );
                            }
                        });
                    ui.end_row();

                    ui.label(self.tr("create.loader"));
                    egui::ComboBox::from_id_salt("instance_configuration_loader")
                        .selected_text(self.tr(loader_key(&draft.loader)))
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for (loader, key) in [
                                ("vanilla", "loader.vanilla"),
                                ("fabric", "loader.fabric"),
                                ("forge", "loader.forge"),
                                ("neo_forge", "loader.neo_forge"),
                            ] {
                                ui.selectable_value(
                                    &mut draft.loader,
                                    loader.to_owned(),
                                    self.tr(key),
                                );
                            }
                        });
                    ui.end_row();

                    ui.label(self.tr("create.loader_version"));
                    if draft.loader == "vanilla" {
                        draft.loader_version = None;
                        ui.label("—");
                    } else {
                        let selected = draft
                            .loader_version
                            .clone()
                            .unwrap_or_else(|| self.tr("common.select"));
                        egui::ComboBox::from_id_salt("instance_configuration_loader_version")
                            .selected_text(selected)
                            .width(ui.available_width())
                            .show_ui(ui, |ui| {
                                for version in &self.instance_loader_versions {
                                    ui.selectable_value(
                                        &mut draft.loader_version,
                                        Some(version.version.clone()),
                                        &version.version,
                                    );
                                }
                            });
                    }
                    ui.end_row();
                });
        });
        let runtime_changed = draft.game_version != original.game_version
            || draft.loader != original.loader
            || draft.loader_version != original.loader_version
            || draft.lwjgl_version != original.lwjgl_version;
        let selection_changed =
            draft.game_version != original.game_version || draft.loader != original.loader;
        self.draft_instance = Some(draft);
        if selection_changed {
            if let Some(draft) = &mut self.draft_instance {
                draft.loader_version = None;
            }
            self.request_instance_loader_versions(id);
        }
        let can_save = self
            .draft_instance
            .as_ref()
            .is_some_and(|draft| draft.loader == "vanilla" || draft.loader_version.is_some());
        if runtime_changed && can_save {
            if let Some(draft) = self.draft_instance.clone() {
                self.save_instance_configuration(draft);
            }
        }
    }

    pub(crate) fn save_instance_configuration(&mut self, draft: Instance) {
        match serde_json::to_value(&draft) {
            Ok(value) => self.call("instance.update", value, json!({"instanceId": draft.id})),
            Err(error) => {
                self.notice = Some(self.trf("launch.save_error", &[("error", &error.to_string())]));
            }
        }
    }

    fn instance_modpack_configuration(&mut self, ui: &mut egui::Ui, id: &str) {
        let active_update = self
            .jobs
            .iter()
            .find(|job| {
                job.kind == "import"
                    && job.instance_id() == Some(id)
                    && matches!(job.status.as_str(), "queued" | "running" | "cancelling")
                    && job
                        .checkpoint
                        .as_ref()
                        .and_then(|checkpoint| checkpoint.get("update"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
            })
            .cloned();
        let update_pending = self.pending.values().any(|pending| {
            pending.method == "modpack.update"
                && pending.context.get("instanceId").and_then(Value::as_str) == Some(id)
        });
        if let Some(job) = &active_update {
            let total = job.progress.total.unwrap_or(0);
            let progress = if total > 0 {
                job.progress.completed as f32 / total as f32
            } else {
                0.0
            };
            ui.label(self.tr(job_stage_key(&job.progress.stage)));
            ui.add(egui::ProgressBar::new(progress).show_percentage());
            ui.separator();
        }
        if !self.instance_modpack.loaded {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(self.tr("configuration.loading_modpack"));
            });
        } else if let Some(record) = self.instance_modpack.record.clone() {
            ui.heading(&record.source_name);
            if let Some(source) = record.source.clone() {
                ui.label(self.trf(
                    "configuration.modpack_source",
                    &[("provider", &source.provider)],
                ));
                let current = self
                    .instance_modpack
                    .current_version
                    .clone()
                    .unwrap_or_else(|| {
                        self.tr(if self.instance_modpack.loading {
                            "configuration.loading_version"
                        } else {
                            "configuration.unknown_version"
                        })
                    });
                ui.label(self.trf(
                    "configuration.current_modpack_version",
                    &[("version", &current)],
                ));
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.instance_modpack.loading
                                && active_update.is_none()
                                && !update_pending,
                            egui::Button::new(self.tr("configuration.check_updates")),
                        )
                        .clicked()
                    {
                        self.request_instance_modpack_versions(id);
                    }
                    if self.instance_modpack.loading {
                        ui.spinner();
                    }
                });
                if !self.instance_modpack.versions.is_empty() {
                    let selected = self
                        .instance_modpack
                        .selected_version
                        .as_deref()
                        .and_then(|selected| {
                            self.instance_modpack
                                .versions
                                .iter()
                                .find(|version| version.file_id == selected)
                        })
                        .map(|version| version.version_name.clone())
                        .unwrap_or_else(|| self.tr("common.select"));
                    egui::ComboBox::from_id_salt("instance_modpack_update_version")
                        .selected_text(selected)
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for version in &self.instance_modpack.versions {
                                let label = if version.game_versions.is_empty() {
                                    version.version_name.clone()
                                } else {
                                    format!(
                                        "{} · {}",
                                        version.version_name,
                                        version.game_versions.join(", ")
                                    )
                                };
                                ui.selectable_value(
                                    &mut self.instance_modpack.selected_version,
                                    Some(version.file_id.clone()),
                                    label,
                                );
                            }
                        });
                    if ui
                        .add_enabled(
                            self.instance_modpack.selected_version.is_some()
                                && active_update.is_none()
                                && !update_pending,
                            egui::Button::new(self.tr("configuration.update_modpack")),
                        )
                        .clicked()
                    {
                        self.instance_modpack.confirm_update =
                            self.instance_modpack.selected_version.clone();
                    }
                } else if !self.instance_modpack.loading {
                    ui.label(self.tr("configuration.no_modpack_updates"));
                }
            } else {
                ui.label(self.tr("configuration.local_modpack"));
            }
        } else {
            ui.label(self.tr("configuration.not_modpack"));
        }
        ui.separator();
        ui.heading(self.tr("configuration.installed_mods"));
        let mods = self.mods.get(id).cloned().unwrap_or_default();
        if mods.is_empty() {
            ui.label(self.tr("mods.empty"));
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for installed in mods {
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(&installed.file.project_name);
                        ui.label(&installed.file.version_name);
                        ui.label(if installed.managed {
                            installed.file.provider.clone()
                        } else {
                            self.tr("mods.untracked")
                        });
                        ui.label(self.tr(if installed.enabled {
                            "configuration.mod_enabled"
                        } else {
                            "configuration.mod_disabled"
                        }));
                    });
                    ui.add_space(crate::layout::CARD_LIST_GAP);
                }
            });
        self.modpack_update_confirmation(ui, id);
    }

    pub(crate) fn request_instance_modpack_versions(&mut self, id: &str) {
        let Some(source) = self
            .instance_modpack
            .record
            .as_ref()
            .and_then(|record| record.source.clone())
        else {
            return;
        };
        self.instance_modpack.loading = true;
        self.instance_modpack.versions.clear();
        self.instance_modpack.selected_version = None;
        self.call(
            "modpack.versions",
            json!({
                "provider": source.provider,
                "projectId": source.project_id,
                "gameVersion": Value::Null,
                "loader": Value::Null,
            }),
            json!({"instanceId": id, "configuration": true}),
        );
    }

    fn modpack_update_confirmation(&mut self, ui: &mut egui::Ui, id: &str) {
        let Some(file_id) = self.instance_modpack.confirm_update.clone() else {
            return;
        };
        let mut update = false;
        let mut cancel = false;
        egui::Window::new(self.tr("configuration.update_modpack"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                ui.label(self.tr("configuration.update_warning"));
                ui.add_space(crate::layout::SECTION_GAP);
                ui.horizontal(|ui| {
                    if ui.button(self.tr("action.cancel")).clicked() {
                        cancel = true;
                    }
                    if ui.button(self.tr("configuration.confirm_update")).clicked() {
                        update = true;
                    }
                });
            });
        if cancel {
            self.instance_modpack.confirm_update = None;
        }
        if update {
            self.update_instance_modpack(id, file_id);
        }
    }

    fn update_instance_modpack(&mut self, id: &str, file_id: String) {
        let Some(record) = self.instance_modpack.record.clone() else {
            return;
        };
        let Some(source) = record.source else {
            return;
        };
        self.instance_modpack.confirm_update = None;
        self.call(
            "modpack.update",
            json!({
                "provider": source.provider,
                "projectId": source.project_id,
                "projectName": record.source_name,
                "projectIconUrl": source.project_icon_url,
                "fileId": file_id,
                "gameVersion": Value::Null,
                "loader": Value::Null,
                "name": Value::Null,
                "instanceId": id,
            }),
            json!({"instanceId": id, "configuration": true}),
        );
    }
}

fn nonempty(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn positive_memory(value: &str) -> Option<u32> {
    value.parse::<u32>().ok().filter(|value| *value > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_input_requires_a_positive_integer() {
        assert_eq!(positive_memory("1"), Some(1));
        assert_eq!(positive_memory("4096"), Some(4096));
        assert_eq!(positive_memory("0"), None);
        assert_eq!(positive_memory(""), None);
        assert_eq!(positive_memory("-1"), None);
        assert_eq!(positive_memory("1.5"), None);
        assert_eq!(positive_memory("4294967296"), None);
    }
}
