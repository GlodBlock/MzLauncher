use eframe::egui;

use crate::app::MzApp;
use crate::pages::home::job_row;

impl MzApp {
    pub fn jobs_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("任务中心");
        if ui.button("刷新").clicked() {
            self.call("job.list", serde_json::Value::Null, serde_json::Value::Null);
        }
        ui.separator();
        if self.jobs.is_empty() {
            ui.label("暂无任务记录");
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
                            format!(
                                "错误：{}（{}）{}",
                                error.message,
                                error.code,
                                if error.retryable { "，可重试" } else { "" }
                            ),
                        );
                    }
                    if let Some(result) = &job.result {
                        ui.collapsing("结果详情", |ui| {
                            ui.small(result.to_string());
                        });
                    }
                });
                ui.add_space(4.0);
            }
        });
    }
}
