use std::collections::HashMap;

use eframe::egui;
use serde_json::{json, Value};

use crate::client::{CoreConnection, CoreMessage};
use crate::i18n::I18n;
use crate::model::*;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Instances,
    Mods,
    Modpack,
    Jobs,
    Accounts,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Overview,
    Mods,
    Logs,
    Launch,
}

#[derive(Clone)]
pub struct CreateDialog {
    pub name: String,
    pub game_version: String,
    pub loader: String,
    pub loader_version: String,
    pub include_snapshots: bool,
}

pub struct ModBrowser {
    pub provider: String,
    pub query: String,
    pub target: Option<String>,
    pub results: Vec<ModProject>,
    pub searching: bool,
    pub files: HashMap<String, Vec<ModFile>>,
    pub expanded: Option<String>,
}

pub struct ModpackWizard {
    pub path: String,
    pub name: String,
    pub analysis: Option<ModpackAnalysis>,
    pub analyzing: bool,
}

pub struct Pending {
    pub method: String,
    pub context: Value,
}

pub struct MzApp {
    pub i18n: I18n,
    pub conn: Option<CoreConnection>,
    pub fatal: Option<String>,
    pub notice: Option<String>,
    pub handshake: Option<HandshakeInfo>,
    pub page: Page,
    pub detail_tab: DetailTab,
    pub accounts: Vec<Account>,
    pub instances: Vec<Instance>,
    pub jobs: Vec<Job>,
    pub games: HashMap<String, String>,
    pub exit_codes: HashMap<String, i32>,
    pub settings: Option<AppSettings>,
    pub settings_draft: Option<AppSettings>,
    pub curseforge_key_input: String,
    pub pending: HashMap<String, Pending>,
    pub instance_search: String,
    pub grid_selected: Option<String>,
    pub selected_instance: Option<String>,
    pub create: Option<CreateDialog>,
    pub versions: Vec<VersionSummary>,
    pub loader_versions: Vec<LoaderVersion>,
    pub login: Option<LoginSession>,
    pub logs: HashMap<String, Vec<String>>,
    pub mods: HashMap<String, Vec<InstalledMod>>,
    pub java_candidates: Vec<JavaInfo>,
    pub preflight: HashMap<String, Value>,
    pub confirm_delete: Option<String>,
    pub browser: ModBrowser,
    pub modpack: ModpackWizard,
    pub draft_instance: Option<Instance>,
}

impl MzApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self {
            i18n: I18n::load(),
            conn: None,
            fatal: None,
            notice: None,
            handshake: None,
            page: Page::Home,
            detail_tab: DetailTab::Overview,
            accounts: Vec::new(),
            instances: Vec::new(),
            jobs: Vec::new(),
            games: HashMap::new(),
            exit_codes: HashMap::new(),
            settings: None,
            settings_draft: None,
            curseforge_key_input: String::new(),
            pending: HashMap::new(),
            instance_search: String::new(),
            grid_selected: None,
            selected_instance: None,
            create: None,
            versions: Vec::new(),
            loader_versions: Vec::new(),
            login: None,
            logs: HashMap::new(),
            mods: HashMap::new(),
            java_candidates: Vec::new(),
            preflight: HashMap::new(),
            confirm_delete: None,
            browser: ModBrowser {
                provider: "modrinth".into(),
                query: String::new(),
                target: None,
                results: Vec::new(),
                searching: false,
                files: HashMap::new(),
                expanded: None,
            },
            modpack: ModpackWizard {
                path: String::new(),
                name: String::new(),
                analysis: None,
                analyzing: false,
            },
            draft_instance: None,
        };
        app.connect();
        app
    }

    pub fn tr(&self, key: &str) -> String {
        self.i18n.tr(key)
    }

    pub fn trf(&self, key: &str, params: &[(&str, &str)]) -> String {
        self.i18n.trf(key, params)
    }

    pub fn connect(&mut self) {
        self.fatal = None;
        self.handshake = None;
        match CoreConnection::spawn() {
            Ok(conn) => {
                self.conn = Some(conn);
                self.call(
                    "system.handshake",
                    json!({
                        "clientVersion": env!("CARGO_PKG_VERSION"),
                        "minProtocol": PROTOCOL_VERSION,
                        "maxProtocol": PROTOCOL_VERSION,
                    }),
                    Value::Null,
                );
            }
            Err((key, detail)) => {
                self.fatal = Some(self.trf(&key, &[("detail", &detail)]));
            }
        }
    }

    pub fn call(&mut self, method: &str, params: Value, context: Value) {
        let Some(conn) = &self.conn else {
            self.notice = Some(self.tr("notice.core_disconnected"));
            return;
        };
        match conn.request(method, params) {
            Ok(id) => {
                self.pending.insert(
                    id,
                    Pending {
                        method: method.to_owned(),
                        context,
                    },
                );
            }
            Err(error) => {
                self.fatal = Some(self.trf("error.request_send", &[("detail", &error)]));
            }
        }
    }

    pub fn refresh_snapshots(&mut self) {
        self.call("account.list", Value::Null, Value::Null);
        self.call("instance.list", Value::Null, Value::Null);
        self.call("job.list", Value::Null, Value::Null);
        self.call("settings.get", Value::Null, Value::Null);
    }

    pub fn refresh_instances(&mut self) {
        self.call("instance.list", Value::Null, Value::Null);
    }

    pub fn refresh_mods(&mut self, instance_id: &str) {
        self.call(
            "mod.list",
            json!({"instanceId": instance_id}),
            json!({"instanceId": instance_id}),
        );
    }

    pub fn instance(&self, id: &str) -> Option<&Instance> {
        self.instances.iter().find(|instance| instance.id == id)
    }

    pub fn instance_name(&self, id: &str) -> String {
        self.instance(id)
            .map(|instance| instance.name.clone())
            .unwrap_or_else(|| id.chars().take(8).collect())
    }

    fn poll(&mut self) {
        let Some(conn) = &self.conn else { return };
        let mut messages = Vec::new();
        while let Ok(message) = conn.receiver.try_recv() {
            messages.push(message);
        }
        for message in messages {
            match message {
                CoreMessage::Response { id, result, error } => {
                    let Some(pending) = self.pending.remove(&id) else {
                        continue;
                    };
                    if let Some(error) = error {
                        self.handle_error(&pending, &error);
                    } else {
                        self.handle_result(&pending, result.unwrap_or(Value::Null));
                    }
                }
                CoreMessage::Event(event) => self.handle_event(event),
                CoreMessage::Disconnected(key, detail) => {
                    let reason = self.trf(&key, &[("detail", &detail)]);
                    self.fatal = Some(self.trf("error.disconnected_stale", &[("reason", &reason)]));
                    self.conn = None;
                    self.pending.clear();
                    break;
                }
            }
        }
    }

    fn handle_error(&mut self, pending: &Pending, error: &Value) {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error")
            .to_owned();
        let code = error.get("code").and_then(Value::as_str).unwrap_or("");
        match pending.method.as_str() {
            "system.handshake" => {
                self.fatal =
                    Some(self.trf("error.protocol_incompatible", &[("message", &message)]));
            }
            "mod.search" => self.browser.searching = false,
            "modpack.analyze" => self.modpack.analyzing = false,
            _ => {}
        }
        self.notice = Some(self.trf(
            "notice.request_failed",
            &[
                ("method", &pending.method),
                ("message", &message),
                ("code", code),
            ],
        ));
    }

    fn handle_event(&mut self, event: Value) {
        let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
        let data = event.get("data").cloned().unwrap_or(Value::Null);
        match kind {
            "job" => {
                if let Ok(job) =
                    serde_json::from_value::<Job>(data.get("job").cloned().unwrap_or(Value::Null))
                {
                    self.merge_job(job);
                }
            }
            "game" => {
                if let (Some(id), Some(status)) = (
                    data.get("instanceId").and_then(Value::as_str),
                    data.get("status").and_then(Value::as_str),
                ) {
                    self.games.insert(id.to_owned(), status.to_owned());
                    self.refresh_instances();
                }
            }
            "log" => {
                if let (Some(id), Some(line)) = (
                    data.get("instanceId").and_then(Value::as_str),
                    data.get("line").and_then(Value::as_str),
                ) {
                    let logs = self.logs.entry(id.to_owned()).or_default();
                    logs.push(line.to_owned());
                    if logs.len() > 2000 {
                        let excess = logs.len() - 2000;
                        logs.drain(..excess);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn merge_job(&mut self, job: Job) {
        if let Some(existing) = self.jobs.iter_mut().find(|item| item.id == job.id) {
            *existing = job;
        } else {
            self.jobs.insert(0, job);
        }
    }

    fn handle_result(&mut self, pending: &Pending, result: Value) {
        match pending.method.as_str() {
            "system.handshake" => {
                self.handshake = serde_json::from_value(result).ok();
                self.refresh_snapshots();
            }
            "account.list" => self.accounts = parse_list(result),
            "instance.list" => {
                self.instances = parse_list(result);
                if self.browser.target.is_none() {
                    self.browser.target = self.instances.first().map(|item| item.id.clone());
                }
            }
            "job.list" => {
                self.jobs = parse_list(result);
                self.jobs.reverse();
            }
            "settings.get" | "settings.update" | "settings.reset" => {
                if let Ok(settings) = serde_json::from_value::<AppSettings>(result) {
                    self.settings = Some(settings.clone());
                    self.settings_draft = Some(settings);
                }
            }
            "version.list" => self.versions = parse_list(result),
            "loader.list" => self.loader_versions = parse_list(result),
            "instance.create" => {
                if let Ok(instance) = serde_json::from_value::<Instance>(result) {
                    self.notice =
                        Some(self.trf("notice.instance_created", &[("name", &instance.name)]));
                    self.selected_instance = Some(instance.id);
                    self.create = None;
                    self.refresh_instances();
                }
            }
            "instance.install" | "instance.repair" | "mod.install" | "mod.update"
            | "modpack.import" => {
                if let Ok(job) = serde_json::from_value::<Job>(result) {
                    self.merge_job(job);
                }
                self.notice = Some(self.tr("notice.job_submitted"));
            }
            "instance.delete" => {
                self.notice = Some(self.tr("notice.instance_deleted"));
                self.selected_instance = None;
                self.refresh_instances();
            }
            "instance.copy" | "instance.rename" | "instance.update" => {
                self.refresh_instances();
                self.notice = Some(self.tr("notice.saved"));
            }
            "game.launch" | "game.stop" | "game.status" => {
                if let Ok(snapshot) = serde_json::from_value::<GameSnapshot>(result) {
                    if let Some(code) = snapshot.exit_code {
                        self.exit_codes.insert(snapshot.instance_id.clone(), code);
                    }
                    self.games.insert(snapshot.instance_id, snapshot.status);
                }
            }
            "game.preflight" => {
                if let Some(id) = result.get("instanceId").and_then(Value::as_str) {
                    self.preflight.insert(id.to_owned(), result);
                }
            }
            "game.logs" => {
                if let Some(id) = pending.context.get("instanceId").and_then(Value::as_str) {
                    let lines: Vec<String> = parse_list(result);
                    self.logs.insert(id.to_owned(), lines);
                }
            }
            "account.begin_login" | "account.login_status" => {
                if let Ok(session) = serde_json::from_value::<LoginSession>(result) {
                    if pending.method == "account.begin_login" {
                        let _ = open::that(&session.authorization_url);
                    }
                    let finished = session.status != "waiting";
                    self.login = Some(session);
                    if finished {
                        self.call("account.list", Value::Null, Value::Null);
                    }
                }
            }
            "account.select" | "account.logout" | "account.refresh" => {
                self.call("account.list", Value::Null, Value::Null);
                self.notice = Some(self.tr("notice.account_updated"));
            }
            "java.list" => self.java_candidates = parse_list(result),
            "provider.curseforge_key.set" => {
                self.curseforge_key_input.clear();
                self.notice = Some(self.tr("notice.curseforge_key_saved"));
            }
            "provider.curseforge_key.delete" => {
                self.notice = Some(self.tr("notice.curseforge_key_deleted"));
            }
            "mod.search" => {
                self.browser.results = parse_list(result);
                self.browser.searching = false;
            }
            "mod.files" => {
                if let Some(project) = pending.context.get("projectId").and_then(Value::as_str) {
                    self.browser
                        .files
                        .insert(project.to_owned(), parse_list(result));
                }
            }
            "mod.list" => {
                if let Some(id) = pending.context.get("instanceId").and_then(Value::as_str) {
                    self.mods.insert(id.to_owned(), parse_list(result));
                }
            }
            "mod.set_enabled" | "mod.remove" => {
                if let Some(id) = pending.context.get("instanceId").and_then(Value::as_str) {
                    self.refresh_mods(id);
                }
            }
            "modpack.analyze" => {
                self.modpack.analyzing = false;
                self.modpack.analysis = serde_json::from_value(result).ok();
            }
            _ => {}
        }
    }
}

fn parse_list<T: serde::de::DeserializeOwned>(value: Value) -> Vec<T> {
    serde_json::from_value(value).unwrap_or_default()
}

impl eframe::App for MzApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        if self
            .login
            .as_ref()
            .is_some_and(|session| session.status == "waiting")
        {
            if let Some(session) = self.login.clone() {
                self.call(
                    "account.login_status",
                    json!({"id": session.id}),
                    Value::Null,
                );
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(500));

        egui::SidePanel::left("nav")
            .resizable(false)
            .exact_width(168.0)
            .show(ctx, |ui| {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    ui.heading(egui::RichText::new("MzLauncher").size(22.0));
                });
                ui.add_space(16.0);
                for (page, key) in [
                    (Page::Home, "nav.home"),
                    (Page::Instances, "nav.instances"),
                    (Page::Mods, "nav.mods"),
                    (Page::Modpack, "nav.modpack"),
                    (Page::Jobs, "nav.jobs"),
                    (Page::Accounts, "nav.accounts"),
                    (Page::Settings, "nav.settings"),
                ] {
                    let selected = self.page == page;
                    if ui
                        .add_sized(
                            [ui.available_width(), 34.0],
                            egui::SelectableLabel::new(
                                selected,
                                egui::RichText::new(self.tr(key)).size(15.0),
                            ),
                        )
                        .clicked()
                    {
                        self.page = page;
                    }
                    ui.add_space(2.0);
                }
            });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(handshake) = &self.handshake {
                    ui.label(self.trf(
                        "status.core_protocol",
                        &[
                            ("version", &handshake.core_version),
                            ("protocol", &handshake.protocol_version.to_string()),
                        ],
                    ));
                } else {
                    ui.label(self.tr("status.core_offline"));
                }
                if let Some(conn) = &self.conn {
                    ui.separator();
                    ui.small(self.trf(
                        "status.core_path",
                        &[("path", &conn.core_path.display().to_string())],
                    ));
                }
                let active = self
                    .jobs
                    .iter()
                    .filter(|job| matches!(job.status.as_str(), "queued" | "running"))
                    .count();
                if active > 0 {
                    ui.separator();
                    ui.label(self.trf("status.active_jobs", &[("count", &active.to_string())]));
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(fatal) = self.fatal.clone() {
                ui.colored_label(egui::Color32::from_rgb(200, 60, 60), fatal);
                if ui.button(self.tr("action.reconnect")).clicked() {
                    self.connect();
                }
                ui.separator();
            }
            if let Some(notice) = self.notice.clone() {
                ui.horizontal(|ui| {
                    ui.label(&notice);
                    if ui.small_button("×").clicked() {
                        self.notice = None;
                    }
                });
                ui.separator();
            }
            match self.page {
                Page::Home => self.home_page(ui),
                Page::Instances => self.instances_page(ui),
                Page::Mods => self.mods_page(ui),
                Page::Modpack => self.modpack_page(ui),
                Page::Jobs => self.jobs_page(ui),
                Page::Accounts => self.accounts_page(ui),
                Page::Settings => self.settings_page(ui),
            }
        });
    }
}
