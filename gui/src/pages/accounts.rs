use eframe::egui;
use serde_json::{json, Value};

use crate::app::MzApp;
use crate::pages::home::account_kind_key;
use crate::skin::show_character;

const ACCOUNT_CARD_WIDTH: f32 = 240.0;
const ACCOUNT_CARD_MIN_HEIGHT: f32 = 340.0;
const ACCOUNT_CHARACTER_SIZE: egui::Vec2 = egui::Vec2::new(112.0, 190.0);

impl MzApp {
    pub fn accounts_page(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.tr("accounts.title"));
        ui.label(self.tr("accounts.privacy"));
        ui.add_space(8.0);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));

        let waiting = self
            .login
            .as_ref()
            .is_some_and(|session| session.status == "waiting");
        let default_id = self
            .settings
            .as_ref()
            .and_then(|settings| settings.default_account_id.clone());
        let accounts = self.accounts.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            if accounts.is_empty() {
                ui.label(self.tr("accounts.empty"));
                ui.add_space(8.0);
            }
            ui.horizontal_wrapped(|ui| {
                let mut max_card_height: Option<f32> = None;
                for account in accounts {
                    let cached = self.skins.texture(&account.id);
                    let loading = self.skins.is_loading(&account.id);
                    let skin = match cached {
                        Some(skin) => skin,
                        None => {
                            let mut fallback = self.skins.fallback();
                            fallback.model = account
                                .skin_model
                                .clone()
                                .unwrap_or_else(|| "classic".into());
                            fallback
                        }
                    };
                    let card = crate::card_frame(ui).show(ui, |ui| {
                        ui.set_width(ACCOUNT_CARD_WIDTH);
                        ui.set_min_height(ACCOUNT_CARD_MIN_HEIGHT);
                        ui.with_layout(
                            egui::Layout::top_down(egui::Align::Center)
                                .with_main_align(egui::Align::Min),
                            |ui| {
                                ui.add_space(8.0);
                                show_character(
                                    ui,
                                    ACCOUNT_CHARACTER_SIZE,
                                    &skin,
                                    ui.ctx().input(|input| input.time),
                                );
                                ui.add_space(crate::layout::CARD_LIST_GAP);
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&account.name).size(17.0).strong(),
                                    )
                                    .truncate(),
                                );
                                ui.label(self.tr(account_kind_key(&account.kind)));
                                ui.label(self.tr(if account.verified {
                                    "accounts.verified"
                                } else {
                                    "accounts.unverified"
                                }));
                                if default_id.as_deref() == Some(account.id.as_str()) {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(59, 130, 246),
                                        self.tr("accounts.default"),
                                    );
                                }
                                if loading {
                                    ui.small(self.tr("accounts.skin_loading"));
                                }
                                if self.skins.is_failed(&account.id)
                                    && ui.button(self.tr("accounts.retry_skin")).clicked()
                                {
                                    self.skins.retry(&account.id);
                                    self.request_missing_skins();
                                }
                                ui.add_space(10.0);
                                ui.horizontal(|ui| {
                                    let actions_width = account_actions_width(ui);
                                    let leading_space =
                                        ((ui.available_width() - actions_width) / 2.0).max(0.0);
                                    ui.add_space(leading_space);
                                    if crate::widgets::IconButton::new(
                                        "account-refresh",
                                        include_bytes!("../../assets/refresh-light.png"),
                                        include_bytes!("../../assets/refresh-dark.png"),
                                    )
                                    .show(ui, &self.tr("accounts.refresh_credentials"))
                                    .clicked()
                                    {
                                        self.skins.invalidate(&account.id);
                                        self.call(
                                            "account.refresh",
                                            json!({"id": account.id}),
                                            Value::Null,
                                        );
                                    }
                                    if crate::widgets::IconButton::new(
                                        "account-set-default",
                                        include_bytes!("../../assets/set-default-light.png"),
                                        include_bytes!("../../assets/set-default-dark.png"),
                                    )
                                    .enabled(default_id.as_deref() != Some(account.id.as_str()))
                                    .show(ui, &self.tr("accounts.set_default"))
                                    .clicked()
                                    {
                                        self.call(
                                            "account.select",
                                            json!({"id": account.id}),
                                            Value::Null,
                                        );
                                        self.call("settings.get", Value::Null, Value::Null);
                                    }
                                    if crate::widgets::IconButton::new(
                                        "account-logout",
                                        include_bytes!("../../assets/logout-light.png"),
                                        include_bytes!("../../assets/logout-dark.png"),
                                    )
                                    .show(ui, &self.tr("accounts.logout"))
                                    .clicked()
                                    {
                                        self.call(
                                            "account.logout",
                                            json!({"id": account.id}),
                                            Value::Null,
                                        );
                                    }
                                });
                            },
                        );
                    });
                    let card_height = card.response.rect.height();
                    max_card_height =
                        Some(max_card_height.map_or(card_height, |max| max.max(card_height)));
                    ui.add_space(crate::layout::CARD_LIST_GAP);
                }

                crate::card_frame(ui).show(ui, |ui| {
                    ui.set_width(ACCOUNT_CARD_WIDTH);
                    let content_height = max_card_height
                        .map(|height| height - 2.0 * crate::layout::CARD_INNER_MARGIN as f32)
                        .unwrap_or(ACCOUNT_CARD_MIN_HEIGHT)
                        .max(ACCOUNT_CARD_MIN_HEIGHT);
                    ui.set_min_height(content_height);
                    ui.with_layout(
                        egui::Layout::top_down(egui::Align::Center)
                            .with_main_align(egui::Align::Min),
                        |ui| {
                            let actions_height = add_account_actions_height(ui);
                            let leading_space = ((content_height - actions_height) / 2.0).max(0.0);
                            ui.add_space(leading_space);
                            ui.heading(self.tr("accounts.add_account_short"));
                            if ui
                                .add_enabled(!waiting, egui::Button::new(self.tr("accounts.login")))
                                .clicked()
                            {
                                self.login = None;
                                self.call("account.begin_login", Value::Null, Value::Null);
                            }
                        },
                    );
                });
            });
            if let Some(session) = self.login.clone() {
                ui.add_space(crate::layout::SECTION_GAP);
                crate::card_frame(ui).show(ui, |ui| {
                    ui.vertical_centered(|ui| match session.status.as_str() {
                        "waiting" => {
                            ui.label(self.tr("accounts.waiting"));
                            ui.horizontal_wrapped(|ui| {
                                ui.label(self.tr("accounts.browser_hint"));
                                if ui.button(self.tr("accounts.open_browser")).clicked() {
                                    let _ = open::that(&session.authorization_url);
                                }
                            });
                            if ui.button(self.tr("accounts.cancel_login")).clicked() {
                                self.call(
                                    "account.cancel_login",
                                    json!({"id": session.id}),
                                    Value::Null,
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
                    });
                });
            }
        });
    }
}

fn add_account_actions_height(ui: &egui::Ui) -> f32 {
    let heading_height = ui.text_style_height(&egui::TextStyle::Heading);
    let button_height =
        ui.text_style_height(&egui::TextStyle::Button) + ui.spacing().button_padding.y * 2.0;
    heading_height + ui.spacing().item_spacing.y + button_height
}

fn account_actions_width(ui: &egui::Ui) -> f32 {
    let action_count = 3.0;
    crate::layout::TOOLBAR_CONTROL_SIZE.x * action_count
        + ui.spacing().item_spacing.x * (action_count - 1.0)
}
