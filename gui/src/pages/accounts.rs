use eframe::egui;
use serde_json::json;

use crate::app::MzApp;
use crate::pages::home::account_kind_key;

impl MzApp {
    pub fn accounts_page(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.tr("accounts.title"));
        ui.label(self.tr("accounts.privacy"));
        ui.add_space(8.0);
        let waiting = self
            .login
            .as_ref()
            .is_some_and(|session| session.status == "waiting");
        if ui
            .add_enabled(!waiting, egui::Button::new(self.tr("accounts.login")))
            .clicked()
        {
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
                    ui.label(self.tr("accounts.waiting"));
                    ui.horizontal(|ui| {
                        ui.label(self.tr("accounts.browser_hint"));
                        if ui.button(self.tr("accounts.open_browser")).clicked() {
                            let _ = open::that(&session.authorization_url);
                        }
                    });
                    if ui.button(self.tr("accounts.cancel_login")).clicked() {
                        self.call(
                            "account.cancel_login",
                            json!({"id": session.id}),
                            serde_json::Value::Null,
                        );
                        self.login = None;
                    }
                }
                "completed" => {
                    ui.label(self.tr("accounts.login_success"));
                    if let Some(account_id) = &session.account_id {
                        ui.small(self.trf("accounts.id", &[("id", account_id)]));
                    }
                }
                status => {
                    ui.colored_label(
                        egui::Color32::from_rgb(200, 60, 60),
                        self.trf("accounts.login_failed_log", &[("status", status)]),
                    );
                }
            }
        }
        ui.separator();
        ui.heading(self.tr("accounts.existing"));
        if self.accounts.is_empty() {
            ui.label(self.tr("accounts.empty"));
        }
        let accounts = self.accounts.clone();
        for account in accounts {
            ui.horizontal(|ui| {
                ui.strong(&account.name);
                ui.label(self.tr(account_kind_key(&account.kind)));
                ui.label(self.tr(if account.verified {
                    "accounts.verified"
                } else {
                    "accounts.unverified"
                }));
                if ui.button(self.tr("accounts.set_default")).clicked() {
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
                if ui.button(self.tr("accounts.refresh_credentials")).clicked() {
                    self.call(
                        "account.refresh",
                        json!({"id": account.id}),
                        serde_json::Value::Null,
                    );
                }
                if ui.button(self.tr("accounts.logout")).clicked() {
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
