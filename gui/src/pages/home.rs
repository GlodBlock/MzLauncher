use eframe::egui;
use serde_json::json;

use crate::app::{DetailTab, MzApp, Page};

impl MzApp {
    pub fn home_page(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.tr("home.title"));
        ui.add_space(8.0);

        let account = self
            .settings
            .as_ref()
            .and_then(|settings| settings.default_account_id.clone())
            .and_then(|id| {
                self.accounts
                    .iter()
                    .find(|account| account.id == id)
                    .cloned()
            })
            .or_else(|| self.accounts.first().cloned());
        ui.horizontal_wrapped(|ui| {
            ui.label(self.tr("home.current_account"));
            match account {
                Some(account) => {
                    ui.strong(account.name);
                    ui.label(self.tr(account_kind_key(&account.kind)));
                }
                None => {
                    ui.label(self.tr("home.no_account"));
                    if ui.button(self.tr("home.add_account")).clicked() {
                        self.page = Page::Accounts;
                    }
                }
            }
        });
        ui.add_space(crate::layout::SECTION_GAP);

        ui.heading(self.tr("home.instances"));
        if self.instances.is_empty() {
            ui.label(self.tr("home.empty_instances"));
            if ui.button(self.tr("action.new_instance")).clicked() {
                self.open_create_dialog();
            }
        }
        let instances = self.instances.clone();
        for instance in instances.iter().take(6) {
            ui.horizontal_wrapped(|ui| {
                ui.strong(&instance.name);
                ui.label(format!(
                    "{} · {}",
                    instance.game_version,
                    self.tr(loader_key(&instance.loader))
                ));
                ui.label(self.tr(status_key(
                    &instance.install_status,
                    self.games.get(&instance.id).map(String::as_str),
                )));
                self.instance_buttons(ui, &instance.id, &instance.install_status);
                if ui.button(self.tr("action.details")).clicked() {
                    self.selected_instance = Some(instance.id.clone());
                    self.detail_tab = DetailTab::Overview;
                    self.page = Page::Instances;
                }
            });
        }
        ui.add_space(crate::layout::SECTION_GAP);

        ui.heading(self.tr("home.active_jobs"));
        let active: Vec<_> = self
            .jobs
            .iter()
            .filter(|job| matches!(job.status.as_str(), "queued" | "running" | "cancelling"))
            .cloned()
            .collect();
        if active.is_empty() {
            ui.label(self.tr("home.no_jobs"));
        }
        for job in active.iter().take(4) {
            job_row(ui, job, self);
        }
        if !active.is_empty() && ui.button(self.tr("home.open_jobs")).clicked() {
            self.page = Page::Jobs;
        }
    }

    pub fn instance_buttons(&mut self, ui: &mut egui::Ui, id: &str, install_status: &str) {
        let running = matches!(
            self.games.get(id).map(String::as_str),
            Some("running" | "starting")
        );
        if running {
            if ui.button(self.tr("action.stop")).clicked() {
                self.call("game.stop", json!({"id": id}), json!({"instanceId": id}));
            }
        } else if install_status == "installed" {
            if ui.button(self.tr("action.launch")).clicked() {
                self.call("game.launch", json!({"id": id}), json!({"instanceId": id}));
            }
        } else if ui.button(self.tr("action.install")).clicked() {
            self.call(
                "instance.install",
                json!({"id": id}),
                json!({"instanceId": id}),
            );
        }
    }
}

pub fn account_kind_key(kind: &str) -> &'static str {
    if kind == "microsoft" {
        "account.kind.microsoft"
    } else {
        "account.kind.local"
    }
}

pub fn loader_key(loader: &str) -> &'static str {
    match loader {
        "fabric" => "loader.fabric",
        "forge" => "loader.forge",
        "neo_forge" => "loader.neo_forge",
        "quilt" => "loader.quilt",
        _ => "loader.vanilla",
    }
}

pub fn status_key(install_status: &str, game: Option<&str>) -> &'static str {
    match game {
        Some("running") => "status.running",
        Some("starting") => "status.starting",
        Some("stopping") => "status.stopping",
        Some("failed") => "status.launch_failed",
        _ => match install_status {
            "installed" => "status.ready",
            "installing" => "status.installing",
            "incomplete" => "status.incomplete",
            "broken" => "status.broken",
            _ => "status.not_installed",
        },
    }
}

pub fn job_kind_key(kind: &str) -> &'static str {
    match kind {
        "install" => "job.kind.install",
        "import" => "job.kind.import",
        "mod_update" => "job.kind.mod_update",
        "download" => "job.kind.download",
        "cache_clean" => "job.kind.cache_clean",
        "data_migration" => "job.kind.data_migration",
        _ => "job.kind.generic",
    }
}

pub fn job_status_key(status: &str) -> &'static str {
    match status {
        "queued" => "job.status.queued",
        "running" => "job.status.running",
        "cancelling" => "job.status.cancelling",
        "cancelled" => "job.status.cancelled",
        "failed" => "job.status.failed",
        "succeeded" => "job.status.succeeded",
        "interrupted" => "job.status.interrupted",
        _ => "job.status.unknown",
    }
}

const JOB_PROGRESS_WIDTH: f32 = 140.0;

pub fn job_row(ui: &mut egui::Ui, job: &crate::model::Job, app: &mut MzApp) {
    ui.horizontal_wrapped(|ui| {
        let target = job
            .target
            .as_deref()
            .map(|id| app.instance_name(id))
            .unwrap_or_default();
        ui.add(
            egui::Label::new(format!(
                "{} · {} · {}",
                app.tr(job_kind_key(&job.kind)),
                target,
                app.tr(job_status_key(&job.status))
            ))
            .wrap(),
        );
        if let Some(total) = job.progress.total {
            if total > 0 {
                let fraction = job.progress.completed as f32 / total as f32;
                ui.add(
                    egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
                        .desired_width(JOB_PROGRESS_WIDTH.min(ui.available_width()))
                        .text(format!("{}/{}", job.progress.completed, total)),
                );
            }
        }
        ui.add(egui::Label::new(&job.progress.stage).wrap());
        if matches!(job.status.as_str(), "queued" | "running")
            && ui.small_button(app.tr("action.cancel")).clicked()
        {
            app.call("job.cancel", json!({"id": job.id}), serde_json::Value::Null);
        }
    });
    if let Some(error) = &job.error {
        let suffix = if error.retryable {
            app.tr("job.retryable_suffix")
        } else {
            String::new()
        };
        ui.colored_label(
            egui::Color32::from_rgb(200, 60, 60),
            format!(
                "{}{}{}",
                app.trf(
                    "job.error",
                    &[("message", &error.message), ("code", &error.code)]
                ),
                if suffix.is_empty() { "" } else { " " },
                suffix
            ),
        );
    }
}
