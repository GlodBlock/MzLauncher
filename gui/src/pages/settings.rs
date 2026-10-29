use eframe::egui;
use serde_json::Value;

use crate::app::MzApp;
use crate::preferences::ThemePreference;

impl MzApp {
    pub fn settings_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(self.tr("settings.title"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(self.tr("action.refresh")).clicked() {
                    self.call("settings.get", Value::Null, Value::Null);
                }
            });
        });
        let Some(mut draft) = self.settings_draft.clone() else {
            ui.label(self.tr("settings.loading"));
            return;
        };
        let mut preferences = self.preferences.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            crate::card_frame(ui).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.heading(self.tr("settings.launcher"));
                egui::Grid::new("settings_launcher")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label(self.tr("settings.language"));
                        let current = self.i18n.current().to_owned();
                        egui::ComboBox::from_id_salt("language")
                            .selected_text(
                                self.i18n
                                    .available()
                                    .into_iter()
                                    .find(|(id, _)| *id == current)
                                    .map(|(_, name)| name)
                                    .unwrap_or(current.clone()),
                            )
                            .show_ui(ui, |ui| {
                                for (id, name) in self.i18n.available() {
                                    if ui.selectable_label(current == id, &name).clicked() {
                                        self.i18n.set_current(&id);
                                        preferences.language = id;
                                        if let Err(error) = preferences.save() {
                                            self.notice = Some(self.trf(
                                                "settings.preferences_save_error",
                                                &[("error", &error)],
                                            ));
                                        }
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label(self.tr("settings.theme"));
                        egui::ComboBox::from_id_salt("theme")
                            .selected_text(self.tr(theme_key(preferences.theme)))
                            .show_ui(ui, |ui| {
                                for theme in [
                                    ThemePreference::Light,
                                    ThemePreference::Dark,
                                    ThemePreference::System,
                                ] {
                                    if ui
                                        .selectable_label(
                                            preferences.theme == theme,
                                            self.tr(theme_key(theme)),
                                        )
                                        .clicked()
                                    {
                                        preferences.theme = theme;
                                        crate::apply_theme(ui.ctx(), theme, self.system_dark);
                                        if let Err(error) = preferences.save() {
                                            self.notice = Some(self.trf(
                                                "settings.preferences_save_error",
                                                &[("error", &error)],
                                            ));
                                        }
                                    }
                                }
                            });
                        ui.end_row();
                    });
            });
            ui.add_space(crate::layout::CARD_LIST_GAP);

            crate::card_frame(ui).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.heading(self.tr("settings.network"));
                egui::Grid::new("settings_network")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label(self.tr("settings.concurrency"));
                        ui.add(
                            egui::DragValue::new(&mut draft.download.max_concurrent).range(1..=32),
                        );
                        ui.end_row();

                        ui.label(self.tr("settings.retries"));
                        ui.add(egui::DragValue::new(&mut draft.download.max_retries).range(0..=10));
                        ui.end_row();

                        ui.label(self.tr("settings.connect_timeout"));
                        ui.add(
                            egui::DragValue::new(&mut draft.download.connect_timeout_seconds)
                                .range(1..=120),
                        );
                        ui.end_row();

                        ui.label(self.tr("settings.read_timeout"));
                        ui.add(
                            egui::DragValue::new(&mut draft.download.read_timeout_seconds)
                                .range(1..=600),
                        );
                        ui.end_row();

                        self.optional_text_row(ui, "settings.proxy", &mut draft.download.proxy_url);
                    });
            });
            ui.add_space(crate::layout::CARD_LIST_GAP);

            crate::card_frame(ui).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.heading(self.tr("settings.java"));
                egui::Grid::new("settings_java")
                    .num_columns(2)
                    .show(ui, |ui| {
                        self.optional_text_row(
                            ui,
                            "settings.global_java_path",
                            &mut draft.launch.java_path,
                        );

                        ui.label(self.tr("launch.min_memory"));
                        ui.add(
                            egui::DragValue::new(&mut draft.launch.min_memory_mb)
                                .range(256..=65536),
                        );
                        ui.end_row();

                        ui.label(self.tr("launch.max_memory"));
                        ui.add(
                            egui::DragValue::new(&mut draft.launch.max_memory_mb)
                                .range(256..=65536),
                        );
                        ui.end_row();

                        ui.label(self.tr("launch.jvm_args"));
                        let mut args = draft.launch.jvm_args.join(" ");
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut args)
                                    .desired_width(ui.available_width()),
                            )
                            .changed()
                        {
                            draft.launch.jvm_args =
                                args.split_whitespace().map(str::to_owned).collect();
                        }
                        ui.end_row();

                        self.optional_text_row(ui, "settings.runtime_dir", &mut draft.runtime_dir);
                    });
                if ui.button(self.tr("settings.scan_java")).clicked() {
                    self.call("java.list", Value::Null, Value::Null);
                }
                for java in &self.java_candidates {
                    ui.label(self.trf(
                        "settings.java_entry",
                        &[
                            ("version", &java.major_version.to_string()),
                            ("arch", &java.architecture),
                            (
                                "managed",
                                &if java.managed {
                                    self.tr("settings.java_managed")
                                } else {
                                    String::new()
                                },
                            ),
                            ("path", &java.path),
                        ],
                    ));
                }
            });
            ui.add_space(crate::layout::CARD_LIST_GAP);

            crate::card_frame(ui).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.heading(self.tr("settings.paths"));
                egui::Grid::new("settings_paths")
                    .num_columns(2)
                    .show(ui, |ui| {
                        self.optional_text_row(
                            ui,
                            "settings.storage_dir",
                            &mut preferences.storage_dir,
                        );
                        self.optional_text_row(ui, "settings.log_dir", &mut preferences.log_dir);
                    });
                ui.label(self.tr("settings.paths_no_migration"));
                if ui
                    .button(self.tr("settings.save_paths_reconnect"))
                    .clicked()
                {
                    self.preferences = preferences.clone();
                    match self.preferences.save() {
                        Ok(()) => {
                            self.notice = Some(self.tr("notice.paths_saved_reconnect"));
                            self.connect();
                        }
                        Err(error) => {
                            self.notice = Some(
                                self.trf("settings.preferences_save_error", &[("error", &error)]),
                            );
                        }
                    }
                }
                ui.separator();
                self.path_row(ui, "settings.data_directory", self.core_data_dir.clone());
                self.path_row(
                    ui,
                    "settings.instance_directory",
                    self.core_instance_dir.clone(),
                );
                if let Some(directory) = self.core_log_dir.clone() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(self.trf("settings.core_log", &[("path", &directory)]));
                        if ui.button(self.tr("settings.open_logs")).clicked() {
                            let _ = open::that(&directory);
                        }
                    });
                }
                if let Some(path) = crate::log::path() {
                    let display = path.display().to_string();
                    ui.horizontal_wrapped(|ui| {
                        ui.label(self.trf("settings.gui_log", &[("path", &display)]));
                        if ui.button(self.tr("settings.open_logs")).clicked() {
                            if let Some(parent) = path.parent() {
                                let _ = open::that(parent);
                            }
                        }
                    });
                }
            });
            ui.add_space(crate::layout::CARD_LIST_GAP);

            crate::card_frame(ui).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.heading(self.tr("settings.api_mirrors"));
                egui::Grid::new("settings_api_mirrors")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label(self.tr("settings.curseforge_key"));
                        ui.horizontal(|ui| {
                            let save_label = self.tr("settings.save_key");
                            let delete_label = self.tr("settings.delete_key");
                            let actions_width = crate::layout::button_width(ui, &save_label)
                                + crate::layout::button_width(ui, &delete_label)
                                + ui.spacing().item_spacing.x * 2.0;
                            let field_width = (ui.available_width() - actions_width).max(0.0);
                            ui.add(
                                egui::TextEdit::singleline(&mut self.curseforge_key_input)
                                    .password(true)
                                    .desired_width(field_width),
                            );
                            if ui
                                .add_enabled(
                                    !self.curseforge_key_input.trim().is_empty(),
                                    egui::Button::new(save_label),
                                )
                                .clicked()
                            {
                                self.call(
                                    "provider.curseforge_key.set",
                                    serde_json::json!({"key": self.curseforge_key_input.trim()}),
                                    Value::Null,
                                );
                            }
                            if ui.button(delete_label).clicked() {
                                self.call(
                                    "provider.curseforge_key.delete",
                                    Value::Null,
                                    Value::Null,
                                );
                            }
                        });
                        ui.end_row();

                        self.optional_text_row(
                            ui,
                            "settings.client_id",
                            &mut draft.microsoft_client_id,
                        );
                        for (key, field) in [
                            (
                                "settings.endpoint.version_manifest",
                                &mut draft.endpoints.version_manifest,
                            ),
                            (
                                "settings.endpoint.java_runtime",
                                &mut draft.endpoints.java_runtime_manifest,
                            ),
                            ("settings.endpoint.fabric", &mut draft.endpoints.fabric_meta),
                            ("settings.endpoint.forge", &mut draft.endpoints.forge_maven),
                            (
                                "settings.endpoint.neoforge",
                                &mut draft.endpoints.neoforge_maven,
                            ),
                            (
                                "settings.endpoint.modrinth",
                                &mut draft.endpoints.modrinth_api,
                            ),
                            (
                                "settings.endpoint.curseforge",
                                &mut draft.endpoints.curseforge_api,
                            ),
                        ] {
                            ui.label(self.tr(key));
                            ui.add(
                                egui::TextEdit::singleline(field)
                                    .desired_width(ui.available_width()),
                            );
                            ui.end_row();
                        }
                    });
            });
            ui.add_space(crate::layout::CARD_LIST_GAP);

            crate::card_frame(ui).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    if ui.button(self.tr("settings.save")).clicked() {
                        match serde_json::to_value(&draft) {
                            Ok(value) => self.call("settings.update", value, Value::Null),
                            Err(error) => {
                                self.notice = Some(
                                    self.trf("launch.save_error", &[("error", &error.to_string())]),
                                )
                            }
                        }
                    }
                    if ui.button(self.tr("settings.reset")).clicked() {
                        self.call("settings.reset", Value::Null, Value::Null);
                    }
                });
            });
            ui.add_space(crate::layout::CARD_LIST_GAP);

            crate::card_frame(ui).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.heading(self.tr("settings.about"));
                ui.label(self.trf(
                    "settings.gui_version",
                    &[("version", env!("CARGO_PKG_VERSION"))],
                ));
                if let Some(conn) = &self.conn {
                    ui.small(self.trf(
                        "status.core_path",
                        &[("path", &conn.core_path.display().to_string())],
                    ));
                }
                if let Some(handshake) = &self.handshake {
                    ui.label(self.trf(
                        "settings.core_versions",
                        &[
                            ("core", &handshake.core_version),
                            ("protocol", &handshake.protocol_version.to_string()),
                            ("data", &handshake.data_version.to_string()),
                        ],
                    ));
                    ui.label(self.trf("settings.platform", &[("platform", &handshake.platform)]));
                    ui.label(self.tr("settings.license"));
                    ui.collapsing(self.tr("settings.capabilities"), |ui| {
                        ui.label(handshake.capabilities.join(", "));
                    });
                }
            });
        });
        self.preferences = preferences;
        self.settings_draft = Some(draft);
    }

    fn optional_text_row(&mut self, ui: &mut egui::Ui, key: &str, value: &mut Option<String>) {
        ui.label(self.tr(key));
        let mut text = value.clone().unwrap_or_default();
        if ui
            .add(egui::TextEdit::singleline(&mut text).desired_width(ui.available_width()))
            .changed()
        {
            *value = if text.trim().is_empty() {
                None
            } else {
                Some(text.trim().to_owned())
            };
        }
        ui.end_row();
    }

    fn path_row(&self, ui: &mut egui::Ui, key: &str, path: Option<String>) {
        let Some(path) = path else { return };
        ui.horizontal_wrapped(|ui| {
            ui.label(self.trf(key, &[("path", &path)]));
            if ui.button(self.tr("settings.open_logs")).clicked() {
                let _ = open::that(path);
            }
        });
    }
}

fn theme_key(theme: ThemePreference) -> &'static str {
    match theme {
        ThemePreference::Light => "settings.theme.light",
        ThemePreference::Dark => "settings.theme.dark",
        ThemePreference::System => "settings.theme.system",
    }
}
