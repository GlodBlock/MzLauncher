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
                crate::card_frame(ui).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    job_row(ui, job, self);
                    if let Some(item) = &job.progress.current_item {
                        ui.add(egui::Label::new(egui::RichText::new(item).small()).wrap());
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
                            ui.add(
                                egui::Label::new(egui::RichText::new(result.to_string()).small())
                                    .wrap(),
                            );
                        });
                    }
                });
                ui.add_space(crate::layout::CARD_LIST_GAP);
            }
        });
    }
}
