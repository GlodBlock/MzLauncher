use eframe::egui;
use serde_json::Value;

use crate::app::MzApp;

impl MzApp {
    pub fn settings_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("设置");
        if ui.button("重新加载").clicked() {
            self.call("settings.get", Value::Null, Value::Null);
        }
        let Some(mut draft) = self.settings_draft.clone() else {
            ui.label("正在读取设置……");
            return;
        };
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("下载");
            ui.horizontal(|ui| {
                ui.label("并发数：");
                ui.add(egui::DragValue::new(&mut draft.download.max_concurrent).range(1..=32));
                ui.label("失败重试：");
                ui.add(egui::DragValue::new(&mut draft.download.max_retries).range(0..=10));
            });
            ui.horizontal(|ui| {
                ui.label("连接超时（秒）：");
                ui.add(
                    egui::DragValue::new(&mut draft.download.connect_timeout_seconds)
                        .range(1..=120),
                );
                ui.label("读取超时（秒）：");
                ui.add(
                    egui::DragValue::new(&mut draft.download.read_timeout_seconds).range(1..=600),
                );
            });
            optional_text(
                ui,
                "代理（http/https，留空禁用）：",
                &mut draft.download.proxy_url,
                420.0,
            );

            ui.add_space(8.0);
            ui.heading("存储");
            optional_text(
                ui,
                "Java 运行时目录（绝对路径，留空使用默认）：",
                &mut draft.runtime_dir,
                420.0,
            );

            ui.add_space(8.0);
            ui.heading("镜像源");
            for (label, field) in [
                ("版本清单", &mut draft.endpoints.version_manifest),
                (
                    "Java 运行时清单",
                    &mut draft.endpoints.java_runtime_manifest,
                ),
                ("Fabric Meta", &mut draft.endpoints.fabric_meta),
                ("Forge Maven", &mut draft.endpoints.forge_maven),
                ("NeoForge Maven", &mut draft.endpoints.neoforge_maven),
                ("Modrinth API", &mut draft.endpoints.modrinth_api),
                ("CurseForge API", &mut draft.endpoints.curseforge_api),
            ] {
                ui.horizontal(|ui| {
                    ui.label(format!("{label}："));
                    ui.add(egui::TextEdit::singleline(field).desired_width(420.0));
                });
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("保存设置").clicked() {
                    match serde_json::to_value(&draft) {
                        Ok(value) => self.call("settings.update", value, Value::Null),
                        Err(error) => self.notice = Some(format!("无法保存：{error}")),
                    }
                }
                if ui.button("恢复默认设置").clicked() {
                    self.call("settings.reset", Value::Null, Value::Null);
                }
            });

            ui.add_space(8.0);
            ui.heading("Java");
            if ui.button("扫描本机 Java").clicked() {
                self.call("java.list", Value::Null, Value::Null);
            }
            for java in &self.java_candidates {
                ui.label(format!(
                    "Java {}（{}{}）：{}",
                    java.major_version,
                    java.architecture,
                    if java.managed { "，自动管理" } else { "" },
                    java.path
                ));
            }

            ui.add_space(8.0);
            ui.heading("关于");
            ui.label(format!("gui 版本：{}", env!("CARGO_PKG_VERSION")));
            if let Some(handshake) = &self.handshake {
                ui.label(format!(
                    "core 版本：{} · 协议 {} · 数据 {}",
                    handshake.core_version, handshake.protocol_version, handshake.data_version
                ));
                ui.label(format!("平台：{}", handshake.platform));
                ui.label("本项目以 LGPL-3.0 发布");
                ui.collapsing("core 支持的能力", |ui| {
                    ui.label(handshake.capabilities.join(", "));
                });
            }
        });
        self.settings_draft = Some(draft);
    }
}

fn optional_text(ui: &mut egui::Ui, label: &str, value: &mut Option<String>, width: f32) {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut text = value.clone().unwrap_or_default();
        if ui
            .add(egui::TextEdit::singleline(&mut text).desired_width(width))
            .changed()
        {
            *value = if text.trim().is_empty() {
                None
            } else {
                Some(text.trim().to_owned())
            };
        }
    });
}
