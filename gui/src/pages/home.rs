use eframe::egui;
use serde_json::json;

use crate::app::{DetailTab, MzApp, Page};

impl MzApp {
    pub fn home_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("首页");
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
        ui.horizontal(|ui| {
            ui.label("当前账号：");
            match account {
                Some(account) => {
                    ui.strong(account.name);
                    ui.label(if account.kind == "microsoft" {
                        "微软账号"
                    } else {
                        "本地账号"
                    });
                }
                None => {
                    ui.label("尚未登录");
                    if ui.button("添加微软账号").clicked() {
                        self.page = Page::Accounts;
                    }
                }
            }
        });
        ui.add_space(12.0);

        ui.heading("实例");
        if self.instances.is_empty() {
            ui.label("还没有实例。创建一个实例或导入整合包即可开始。");
            if ui.button("新建实例").clicked() {
                self.open_create_dialog();
            }
        }
        let instances = self.instances.clone();
        for instance in instances.iter().take(6) {
            ui.horizontal(|ui| {
                ui.strong(&instance.name);
                ui.label(format!(
                    "{} · {}",
                    instance.game_version,
                    loader_label(&instance.loader)
                ));
                ui.label(status_label(
                    &instance.install_status,
                    self.games.get(&instance.id).map(String::as_str),
                ));
                self.instance_buttons(ui, &instance.id, &instance.install_status);
                if ui.button("详情").clicked() {
                    self.selected_instance = Some(instance.id.clone());
                    self.detail_tab = DetailTab::Overview;
                    self.page = Page::Instances;
                }
            });
        }
        ui.add_space(12.0);

        ui.heading("进行中的任务");
        let active: Vec<_> = self
            .jobs
            .iter()
            .filter(|job| matches!(job.status.as_str(), "queued" | "running" | "cancelling"))
            .cloned()
            .collect();
        if active.is_empty() {
            ui.label("暂无任务");
        }
        for job in active.iter().take(4) {
            job_row(ui, job, self);
        }
        if !active.is_empty() && ui.button("打开任务中心").clicked() {
            self.page = Page::Jobs;
        }
    }

    pub fn instance_buttons(&mut self, ui: &mut egui::Ui, id: &str, install_status: &str) {
        let running = matches!(
            self.games.get(id).map(String::as_str),
            Some("running" | "starting")
        );
        if running {
            if ui.button("停止").clicked() {
                self.call("game.stop", json!({"id": id}), json!({"instanceId": id}));
            }
        } else {
            if install_status == "installed" && ui.button("启动").clicked() {
                self.call("game.launch", json!({"id": id}), json!({"instanceId": id}));
            }
            if ui.button("安装/修复").clicked() {
                self.call(
                    "instance.install",
                    json!({"id": id}),
                    json!({"instanceId": id}),
                );
            }
        }
    }
}

pub fn loader_label(loader: &str) -> &'static str {
    match loader {
        "fabric" => "Fabric",
        "forge" => "Forge",
        "neo_forge" => "NeoForge",
        "quilt" => "Quilt",
        _ => "Vanilla",
    }
}

pub fn status_label(install_status: &str, game: Option<&str>) -> String {
    match game {
        Some("running") => "运行中".into(),
        Some("starting") => "启动中".into(),
        Some("stopping") => "停止中".into(),
        Some("failed") => "启动失败".into(),
        _ => match install_status {
            "installed" => "可启动".into(),
            "installing" => "安装中".into(),
            "incomplete" => "不完整".into(),
            "broken" => "已损坏".into(),
            _ => "未安装".into(),
        },
    }
}

pub fn job_kind_label(kind: &str) -> &'static str {
    match kind {
        "install" => "实例安装",
        "import" => "整合包导入",
        "mod_update" => "Mod 更新",
        "download" => "下载",
        "cache_clean" => "缓存清理",
        "data_migration" => "数据迁移",
        _ => "任务",
    }
}

pub fn job_status_label(status: &str) -> &'static str {
    match status {
        "queued" => "排队中",
        "running" => "进行中",
        "cancelling" => "取消中",
        "cancelled" => "已取消",
        "failed" => "失败",
        "succeeded" => "已完成",
        "interrupted" => "已中断",
        _ => "未知",
    }
}

pub fn job_row(ui: &mut egui::Ui, job: &crate::model::Job, app: &mut MzApp) {
    ui.horizontal(|ui| {
        ui.label(format!(
            "{} · {} · {}",
            job_kind_label(&job.kind),
            job.target
                .as_deref()
                .map(|id| app.instance_name(id))
                .unwrap_or_default(),
            job_status_label(&job.status)
        ));
        if let Some(total) = job.progress.total {
            if total > 0 {
                let fraction = job.progress.completed as f32 / total as f32;
                ui.add(
                    egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
                        .desired_width(140.0)
                        .text(format!("{}/{}", job.progress.completed, total)),
                );
            }
        }
        ui.label(&job.progress.stage);
        if matches!(job.status.as_str(), "queued" | "running") && ui.small_button("取消").clicked()
        {
            app.call("job.cancel", json!({"id": job.id}), serde_json::Value::Null);
        }
    });
    if let Some(error) = &job.error {
        ui.colored_label(
            egui::Color32::from_rgb(200, 60, 60),
            format!("错误：{}（{}）", error.message, error.code),
        );
    }
}
