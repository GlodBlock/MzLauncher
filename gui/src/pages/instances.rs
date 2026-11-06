use eframe::egui;
use serde_json::{json, Value};

use crate::app::{InstallingModpack, MzApp, NewInstanceState, NewInstanceTab, Page};
use crate::model::{Instance, Job};
use crate::pages::home::{job_stage_key, job_status_key, loader_key, status_key};

const INSTANCE_ICON_SIZE: egui::Vec2 = egui::Vec2::new(48.0, 48.0);
const INSTANCE_ICON_GRASS_HEIGHT: f32 = 15.0;
const INSTANCE_ROW_CONTENT_HEIGHT: f32 = 64.0;
const NEW_INSTANCE_BAR_HEIGHT: f32 = 48.0;
const NEW_INSTANCE_ICON_SIZE: egui::Vec2 = egui::Vec2::new(18.0, 18.0);
const NEW_INSTANCE_ICON_TEXT_GAP: f32 = 8.0;
const INSTALL_ACTION_SIZE: egui::Vec2 = crate::layout::TOOLBAR_CONTROL_SIZE;
const INSTALL_DETAILS_WINDOW_SIZE: [f32; 2] = [560.0, 640.0];
const INSTALL_DETAILS_MIN_WINDOW_SIZE: [f32; 2] = [420.0, 320.0];
const ACTIVE_DOWNLOADS_MAX_HEIGHT: f32 = 160.0;

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
        self.delete_dialog();
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
        select_displayed_installation_job(&self.jobs, &self.installing_modpacks, instance_id)
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

    fn paint_instance_icon(
        &mut self,
        ui: &mut egui::Ui,
        name: &str,
        project: Option<&crate::model::ModProject>,
    ) {
        let texture = project.and_then(|project| {
            self.modpack_icons
                .texture(ui.ctx(), &crate::app::modpack_icon_key(project))
        });
        if let Some(texture) = texture {
            let (rect, _) = ui.allocate_exact_size(INSTANCE_ICON_SIZE, egui::Sense::hover());
            egui::Image::new((texture.id(), INSTANCE_ICON_SIZE))
                .corner_radius(egui::CornerRadius::same(8))
                .paint_at(ui, rect);
        } else {
            instance_icon(ui, name);
        }
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
                    self.paint_instance_icon(ui, name, project);
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
                                        self.tr(job_stage_key(&job.progress.stage))
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
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(job) = job {
                            if delete_instance_button(ui, &self.tr("action.delete_instance"))
                                .clicked()
                            {
                                delete =
                                    Some((job.id.clone(), job.instance_id().map(str::to_owned)));
                            }
                            if install_details_button(ui, &self.tr("action.details")).clicked() {
                                details = Some(job.id.clone());
                            }
                        }
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
                        } else if let Some(job) = job
                            .filter(|job| matches!(job.status.as_str(), "failed" | "interrupted"))
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
                    });
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
        let project = self
            .jobs
            .iter()
            .filter(|job| job.kind == "import" && job.status == "succeeded")
            .find(|job| job.instance_id() == Some(instance.id.as_str()))
            .and_then(InstallingModpack::project_from_job);
        let retry_job = retry_modpack_job(&self.jobs, &instance.id).map(|job| job.id.clone());
        let running = matches!(
            self.games.get(&instance.id).map(String::as_str),
            Some("running" | "starting")
        );
        let installed = instance.install_status == "installed"
            || (instance.install_status == "incomplete" && project.is_some());
        crate::card_frame(ui).show(ui, |ui| {
            let width = ui.available_width();
            ui.set_min_width(width);
            ui.allocate_ui_with_layout(
                egui::vec2(width, INSTANCE_ROW_CONTENT_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    self.paint_instance_icon(ui, &instance.name, project.as_ref());
                    let spacing = ui.spacing().item_spacing.x;
                    let action_count = 4 + usize::from(retry_job.is_some());
                    let action_count = action_count as f32;
                    let info_width = (ui.available_width()
                        - INSTALL_ACTION_SIZE.x * action_count
                        - spacing * action_count)
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
                                &[(
                                    "time",
                                    &format_last_launch(instance.last_launched_at.as_deref()),
                                )],
                            );
                            let played = self.trf(
                                "instances.play_time",
                                &[("time", &format_play_time(instance.total_play_time_ms))],
                            );
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!("{status} · {last} · {played}"))
                                        .small(),
                                )
                                .truncate(),
                            );
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if delete_instance_button(ui, &self.tr("action.delete_instance")).clicked()
                        {
                            self.confirm_delete = Some(crate::app::DeleteConfirmation {
                                instance_id: Some(instance.id.clone()),
                                job_id: None,
                                name: instance.name.clone(),
                            });
                        }
                        let can_open = self.core_instance_dir.is_some();
                        if crate::widgets::IconButton::new(
                            "open-instance-directory",
                            include_bytes!("../../assets/folder-open-light.png"),
                            include_bytes!("../../assets/folder-open-dark.png"),
                        )
                        .enabled(can_open)
                        .show(ui, &self.tr("action.open_directory"))
                        .clicked()
                        {
                            if let Some(directory) = &self.core_instance_dir {
                                let _ = open::that(
                                    std::path::PathBuf::from(directory).join(&instance.id),
                                );
                            }
                        }
                        if crate::widgets::IconButton::new(
                            "configure-instance",
                            include_bytes!("../../assets/settings-light.png"),
                            include_bytes!("../../assets/settings-dark.png"),
                        )
                        .show(ui, &self.tr("action.configure"))
                        .clicked()
                        {
                            self.open_instance_configuration(&instance.id);
                        }
                        if let Some(job_id) = &retry_job {
                            if crate::widgets::IconButton::new(
                                "retry-failed-mods",
                                include_bytes!("../../assets/rotate-right-light.png"),
                                include_bytes!("../../assets/rotate-right-dark.png"),
                            )
                            .show(ui, &self.tr("action.retry_failed_mods"))
                            .clicked()
                            {
                                self.retry_modpack_install(job_id);
                            }
                        }
                        let primary_tooltip = if running {
                            "action.stop_game"
                        } else if installed {
                            "action.launch_game"
                        } else {
                            "action.prepare_install"
                        };
                        let primary = if running {
                            crate::widgets::IconButton::new(
                                "stop-instance",
                                include_bytes!("../../assets/stop-light.png"),
                                include_bytes!("../../assets/stop-dark.png"),
                            )
                            .show(ui, &self.tr(primary_tooltip))
                        } else if installed {
                            crate::widgets::IconButton::new(
                                "launch-instance",
                                include_bytes!("../../assets/play-light.png"),
                                include_bytes!("../../assets/play-dark.png"),
                            )
                            .show(ui, &self.tr(primary_tooltip))
                        } else {
                            crate::widgets::IconButton::new(
                                "install-instance",
                                include_bytes!("../../assets/download-light.png"),
                                include_bytes!("../../assets/download-dark.png"),
                            )
                            .show(ui, &self.tr(primary_tooltip))
                        };
                        if primary.clicked() {
                            if running {
                                self.call(
                                    "game.stop",
                                    json!({"id": instance.id}),
                                    json!({"instanceId": instance.id}),
                                );
                            } else {
                                self.launch_instance(&instance.id, installed);
                            }
                        }
                    });
                },
            );
        });
    }

    fn delete_dialog(&mut self) {
        let Some(confirmation) = self.confirm_delete.take() else {
            return;
        };
        let description = format!(
            "{}\n{}",
            self.trf("delete.confirm", &[("name", &confirmation.name)]),
            self.tr("delete.warning")
        );
        let result = rfd::MessageDialog::new()
            .set_title(self.tr("delete.title"))
            .set_description(description)
            .set_level(rfd::MessageLevel::Warning)
            .set_buttons(rfd::MessageButtons::OkCancel)
            .show();
        if result != rfd::MessageDialogResult::Ok {
            return;
        }
        if let Some(id) = confirmation.instance_id {
            self.call("instance.delete", json!({"id": id}), Value::Null);
        } else if let Some(id) = confirmation.job_id {
            self.call("job.cancel", json!({"id": id}), Value::Null);
        }
    }

    pub(crate) fn install_details_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.install_details_job.clone() else {
            return;
        };
        let Some(job) = self.jobs.iter().find(|job| job.id == id).cloned() else {
            self.install_details_job = None;
            return;
        };
        let active_items = job.progress.active_items();
        let failed_entries = failed_modpack_entries(&job);
        let active_heading = self.trf(
            "install_details.current",
            &[("count", &active_items.len().to_string())],
        );
        let failed_heading = self.trf(
            "install_details.failed",
            &[("count", &failed_entries.len().to_string())],
        );
        let pending_heading = self.trf(
            "install_details.pending",
            &[("count", &job.progress.pending_items.len().to_string())],
        );
        let empty = self.tr("install_details.empty");
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("install_details_window"),
            egui::ViewportBuilder::default()
                .with_title(self.tr("install_details.title"))
                .with_inner_size(INSTALL_DETAILS_WINDOW_SIZE)
                .with_min_inner_size(INSTALL_DETAILS_MIN_WINDOW_SIZE),
            |ctx, _class| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label(egui::RichText::new(&active_heading).strong());
                    if active_items.is_empty() {
                        ui.label(&empty);
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("active_downloads")
                            .auto_shrink([false, true])
                            .max_height(ACTIVE_DOWNLOADS_MAX_HEIGHT)
                            .show(ui, |ui| {
                                for item in &active_items {
                                    ui.add(egui::Label::new(*item).wrap());
                                }
                            });
                    }
                    if !failed_entries.is_empty() {
                        ui.separator();
                        ui.label(egui::RichText::new(&failed_heading).strong());
                        egui::ScrollArea::vertical()
                            .id_salt("failed_downloads")
                            .auto_shrink([false, true])
                            .max_height(ACTIVE_DOWNLOADS_MAX_HEIGHT)
                            .show(ui, |ui| {
                                for entry in &failed_entries {
                                    ui.add(egui::Label::new(&entry.name).wrap());
                                    if let Some(detail) = &entry.detail {
                                        ui.add(
                                            egui::Label::new(egui::RichText::new(detail).small())
                                                .wrap(),
                                        );
                                    }
                                }
                            });
                    }
                    ui.separator();
                    ui.label(egui::RichText::new(&pending_heading).strong());
                    if job.progress.pending_items.is_empty() {
                        ui.label(&empty);
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("pending_downloads")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for item in &job.progress.pending_items {
                                    ui.add(egui::Label::new(item).wrap());
                                }
                            });
                    }
                });
                if ctx.input(|input| input.viewport().close_requested()) {
                    self.install_details_job = None;
                }
            },
        );
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

fn failed_modpack_entries(job: &Job) -> Vec<crate::model::ModpackEntryResult> {
    job.result
        .as_ref()
        .and_then(|result| result.get("entries"))
        .cloned()
        .and_then(|entries| {
            serde_json::from_value::<Vec<crate::model::ModpackEntryResult>>(entries).ok()
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|entry: &crate::model::ModpackEntryResult| {
            matches!(entry.status.as_str(), "failed" | "requires_manual_download")
        })
        .collect()
}

fn retry_modpack_job<'a>(jobs: &'a [Job], instance_id: &str) -> Option<&'a Job> {
    jobs.iter()
        .find(|job| job.kind == "import" && job.instance_id() == Some(instance_id))
        .filter(|job| {
            job.status == "succeeded"
                && job
                    .result
                    .as_ref()
                    .and_then(|result| result.get("complete"))
                    .and_then(Value::as_bool)
                    == Some(false)
        })
}

fn select_displayed_installation_job<'a>(
    jobs: &'a [Job],
    installing: &[InstallingModpack],
    instance_id: &str,
) -> Option<&'a Job> {
    if let Some(job) = installing
        .iter()
        .find(|install| install.instance_id.as_deref() == Some(instance_id))
        .and_then(|install| install.job_id.as_deref())
        .and_then(|id| jobs.iter().find(|job| job.id == id))
        .filter(|job| job.status != "succeeded")
    {
        return Some(job);
    }
    jobs.iter().find(|job| {
        matches!(job.kind.as_str(), "import" | "install")
            && matches!(job.status.as_str(), "queued" | "running" | "cancelling")
            && job.instance_id() == Some(instance_id)
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn import_job(id: &str, status: &str) -> Job {
        serde_json::from_value(json!({
            "id": id,
            "kind": "import",
            "status": status,
            "target": null,
            "progress": {
                "stage": "downloading",
                "completed": 1,
                "total": 2,
                "currentItem": null
            },
            "error": null,
            "result": null,
            "checkpoint": {
                "instanceId": "instance-1",
                "modpack": {
                    "provider": "curse_forge",
                    "projectId": "project-1",
                    "projectName": "Test Pack"
                }
            }
        }))
        .unwrap()
    }

    fn install_for(job: &Job) -> InstallingModpack {
        InstallingModpack {
            project: crate::model::ModProject {
                provider: "curse_forge".into(),
                project_id: "project-1".into(),
                name: "Test Pack".into(),
                summary: String::new(),
                author: String::new(),
                icon_url: None,
                page_url: None,
                downloads: 0,
                game_versions: Vec::new(),
                loaders: Vec::new(),
            },
            file_id: Some("file-1".into()),
            game_version: "1.21.1".into(),
            loader: "neo_forge".into(),
            job_id: Some(job.id.clone()),
            instance_id: job.instance_id().map(str::to_owned),
        }
    }

    #[test]
    fn restored_instance_uses_the_same_job_as_the_retry_list() {
        let older = import_job("older-job", "interrupted");
        let newer = import_job("newer-job", "interrupted");
        let jobs = vec![older, newer.clone()];
        let installing = vec![install_for(&newer)];

        let selected = select_displayed_installation_job(&jobs, &installing, "instance-1").unwrap();
        assert_eq!(selected.id, "newer-job");
    }

    #[test]
    fn completed_modpack_does_not_restore_an_older_interrupted_job() {
        let interrupted = import_job("interrupted-job", "interrupted");
        let mut succeeded = import_job("succeeded-job", "succeeded");
        succeeded.result = Some(json!({"instanceId": "instance-1"}));
        let jobs = vec![interrupted, succeeded];

        assert!(select_displayed_installation_job(&jobs, &[], "instance-1").is_none());
    }

    #[test]
    fn incomplete_success_is_not_displayed_as_an_active_installation() {
        let mut job = import_job("job", "succeeded");
        job.result = Some(json!({"complete": false}));
        let installing = vec![install_for(&job)];

        assert!(select_displayed_installation_job(&[job], &installing, "instance-1").is_none());
    }

    #[test]
    fn complete_repair_hides_retry_from_older_incomplete_job() {
        let mut repaired = import_job("repair-job", "succeeded");
        repaired.result = Some(json!({"complete": true}));
        let mut incomplete = import_job("incomplete-job", "succeeded");
        incomplete.result = Some(json!({"complete": false}));

        assert!(retry_modpack_job(&[repaired, incomplete], "instance-1").is_none());
    }

    #[test]
    fn latest_incomplete_job_offers_retry() {
        let mut incomplete = import_job("incomplete-job", "succeeded");
        incomplete.result = Some(json!({"complete": false}));
        let mut complete = import_job("complete-job", "succeeded");
        complete.result = Some(json!({"complete": true}));

        assert_eq!(
            retry_modpack_job(&[incomplete, complete], "instance-1").map(|job| job.id.as_str()),
            Some("incomplete-job")
        );
    }

    #[test]
    fn download_details_include_failed_mods_and_reasons() {
        let mut job = import_job("job", "succeeded");
        job.result = Some(json!({
            "entries": [
                {"name": "mods/ok.jar", "status": "imported", "detail": null},
                {"name": "mods/failed.jar", "status": "failed", "detail": "network timeout"},
                {
                    "name": "mods/manual.jar",
                    "status": "requires_manual_download",
                    "detail": "automatic download unavailable"
                }
            ]
        }));

        let entries = failed_modpack_entries(&job);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "mods/failed.jar");
        assert_eq!(entries[0].detail.as_deref(), Some("network timeout"));
        assert_eq!(entries[1].name, "mods/manual.jar");
    }
}
