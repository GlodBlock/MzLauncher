use eframe::egui;

use crate::app::MzApp;
use crate::pages::home::job_row;

impl MzApp {
    pub fn jobs_page(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.tr("jobs.title"));
        if ui.button(self.tr("action.refresh")).clicked() {
            self.call("job.list", serde_json::Value::Null, serde_json::Value::Null);
        }
        ui.separator();
        if self.jobs.is_empty() {
            ui.label(self.tr("jobs.empty"));
        }
        let jobs = self.jobs.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for job in &jobs {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    job_row(ui, job, self);
                    if let Some(item) = &job.progress.current_item {
                        ui.small(item);
                    }
                    if let Some(error) = &job.error {
                        ui.colored_label(
                            egui::Color32::from_rgb(200, 60, 60),
                            self.trf(
                                "job.error",
                                &[("message", &error.message), ("code", &error.code)],
                            ),
                        );
                    }
                    if let Some(result) = &job.result {
                        ui.collapsing(self.tr("jobs.result_details"), |ui| {
                            ui.small(result.to_string());
                        });
                    }
                });
                ui.add_space(4.0);
            }
        });
    }
}
