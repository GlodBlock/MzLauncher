use eframe::egui;
use serde_json::json;

use crate::app::MzApp;

impl MzApp {
    pub fn accounts_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("账号");
        ui.label("登录使用微软官方页面完成，MzLauncher 不会要求你输入微软密码。");
        ui.add_space(8.0);
        if ui.button("使用微软账号登录").clicked() {
            self.login = None;
            self.call(
                "account.begin_login",
                serde_json::Value::Null,
                serde_json::Value::Null,
            );
        }
        if let Some(session) = self.login.clone() {
            ui.separator();
            match session.status.as_str() {
                "waiting" => {
                    ui.label("等待你在浏览器中完成授权……");
                    ui.horizontal(|ui| {
                        ui.label("若浏览器未打开，");
                        if ui.button("点击这里").clicked() {
                            let _ = open::that(&session.authorization_url);
                        }
                    });
                    if ui.button("取消登录").clicked() {
                        self.call(
                            "account.cancel_login",
                            json!({"id": session.id}),
                            serde_json::Value::Null,
                        );
                        self.login = None;
                    }
                }
                "completed" => {
                    ui.label("登录成功。");
                    if let Some(account_id) = &session.account_id {
                        ui.small(format!("账号 ID：{account_id}"));
                    }
                }
                status => {
                    ui.colored_label(
                        egui::Color32::from_rgb(200, 60, 60),
                        format!(
                            "登录未完成：{} {}",
                            status,
                            session.error.unwrap_or_default()
                        ),
                    );
                }
            }
        }
        ui.separator();
        ui.heading("已有账号");
        if self.accounts.is_empty() {
            ui.label("暂无账号");
        }
        let accounts = self.accounts.clone();
        for account in accounts {
            ui.horizontal(|ui| {
                ui.strong(&account.name);
                ui.label(if account.kind == "microsoft" {
                    "微软账号"
                } else {
                    "本地账号"
                });
                ui.label(if account.verified {
                    "已验证"
                } else {
                    "未验证"
                });
                if ui.button("设为默认").clicked() {
                    self.call(
                        "account.select",
                        json!({"id": account.id}),
                        serde_json::Value::Null,
                    );
                    self.call(
                        "settings.get",
                        serde_json::Value::Null,
                        serde_json::Value::Null,
                    );
                }
                if ui.button("刷新凭据").clicked() {
                    self.call(
                        "account.refresh",
                        json!({"id": account.id}),
                        serde_json::Value::Null,
                    );
                }
                if ui.button("退出登录").clicked() {
                    self.call(
                        "account.logout",
                        json!({"id": account.id}),
                        serde_json::Value::Null,
                    );
                }
            });
        }
    }
}
