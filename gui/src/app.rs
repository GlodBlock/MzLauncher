use std::collections::HashMap;

use eframe::egui;
use serde_json::{json, Value};

use crate::client::{CoreConnection, CoreMessage};
use crate::i18n::I18n;
use crate::model::*;
use crate::modpack_icons::ModpackIconCache;
use crate::preferences::GuiPreferences;
use crate::skin::SkinCache;

pub const PROTOCOL_VERSION: u32 = 2;
pub const MODPACK_PAGE_SIZE: usize = 20;

#[allow(dead_code)]
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NewInstanceTab {
    CurseForge,
    Ftb,
    Modrinth,
    Custom,
}

impl NewInstanceTab {
    pub fn provider(self) -> Option<&'static str> {
        match self {
            Self::CurseForge => Some("curse_forge"),
            Self::Ftb => Some("ftb"),
            Self::Modrinth => Some("modrinth"),
            Self::Custom => None,
        }
    }
}

#[derive(Clone)]
pub struct NewInstanceState {
    pub name: String,
    pub game_version: String,
    pub loader: String,
    pub loader_version: String,
    pub include_snapshots: bool,
    pub tab: NewInstanceTab,
    pub creating: bool,
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

pub struct ModpackBrowser {
    pub provider: String,
    pub query: String,
    pub game_version: String,
    pub loader: String,
    pub results: Vec<ModProject>,
    pub searching: bool,
    pub generation: u64,
    pub offset: usize,
    pub has_next: bool,
    pub requested: std::collections::HashSet<String>,
    pub install_jobs: HashMap<String, String>,
}

impl ModpackBrowser {
    pub fn new(provider: &str) -> Self {
        Self {
            provider: provider.into(),
            query: String::new(),
            game_version: String::new(),
            loader: String::new(),
            results: Vec::new(),
            searching: false,
            generation: 0,
            offset: 0,
            has_next: false,
            requested: std::collections::HashSet::new(),
            install_jobs: HashMap::new(),
        }
    }
}

#[derive(Clone)]
pub struct InstallingModpack {
    pub project: ModProject,
    pub game_version: String,
    pub loader: String,
    pub job_id: Option<String>,
}

impl InstallingModpack {
    fn from_job(job: &Job) -> Option<Self> {
        if job.kind != "import"
            || !matches!(
                job.status.as_str(),
                "queued" | "running" | "cancelling" | "interrupted" | "failed"
            )
        {
            return None;
        }
        let source = job.checkpoint.as_ref()?.get("modpack")?;
        let project_id = source.get("projectId")?.as_str()?.to_owned();
        let name = source.get("projectName")?.as_str()?.to_owned();
        Some(Self {
            project: ModProject {
                provider: source.get("provider")?.as_str()?.to_owned(),
                project_id,
                name,
                summary: String::new(),
                author: String::new(),
                icon_url: source
                    .get("projectIconUrl")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                page_url: None,
                downloads: 0,
                game_versions: Vec::new(),
                loaders: Vec::new(),
            },
            game_version: source
                .get("gameVersion")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            loader: source
                .get("loader")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            job_id: Some(job.id.clone()),
        })
    }
}

pub struct ModpackWizard {
    pub path: String,
    pub name: String,
    pub analysis: Option<ModpackAnalysis>,
    pub analyzing: bool,
    pub importing: bool,
}

pub struct Pending {
    pub method: String,
    pub context: Value,
}

pub struct MzApp {
    pub i18n: I18n,
    pub preferences: GuiPreferences,
    pub system_dark: bool,
    pub applied_theme: Option<(crate::preferences::ThemePreference, bool)>,
    pub skins: SkinCache,
    pub modpack_icons: ModpackIconCache,
    pub conn: Option<CoreConnection>,
    pub fatal: Option<String>,
    pub notice: Option<String>,
    pub handshake: Option<HandshakeInfo>,
    pub page: Page,
    pub detail_tab: DetailTab,
    pub accounts: Vec<Account>,
    pub instances: Vec<Instance>,
    pub jobs: Vec<Job>,
    pub installing_modpacks: Vec<InstallingModpack>,
    pub games: HashMap<String, String>,
    pub exit_codes: HashMap<String, i32>,
    pub settings: Option<AppSettings>,
    pub settings_draft: Option<AppSettings>,
    pub curseforge_key_input: String,
    pub pending: HashMap<String, Pending>,
    pub instance_search: String,
    pub selected_instance: Option<String>,
    pub create: Option<NewInstanceState>,
    pub versions: Vec<VersionSummary>,
    pub loader_versions: Vec<LoaderVersion>,
    pub login: Option<LoginSession>,
    pub logs: HashMap<String, Vec<String>>,
    pub mods: HashMap<String, Vec<InstalledMod>>,
    pub java_candidates: Vec<JavaInfo>,
    pub preflight: HashMap<String, Value>,
    pub core_data_dir: Option<String>,
    pub core_instance_dir: Option<String>,
    pub core_log_dir: Option<String>,
    pub confirm_delete: Option<String>,
    pub browser: ModBrowser,
    pub modpack_browser: ModpackBrowser,
    pub modpack: ModpackWizard,
    pub draft_instance: Option<Instance>,
}

impl MzApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::log::init();
        crate::log::write("gui starting");
        let preferences = GuiPreferences::load();
        let system_dark = cc.egui_ctx.style().visuals.dark_mode;
        let mut app = Self {
            i18n: I18n::load(&preferences.language),
            preferences,
            system_dark,
            applied_theme: None,
            skins: SkinCache::default(),
            modpack_icons: ModpackIconCache::default(),
            conn: None,
            fatal: None,
            notice: None,
            handshake: None,
            page: Page::Instances,
            detail_tab: DetailTab::Overview,
            accounts: Vec::new(),
            instances: Vec::new(),
            jobs: Vec::new(),
            installing_modpacks: Vec::new(),
            games: HashMap::new(),
            exit_codes: HashMap::new(),
            settings: None,
            settings_draft: None,
            curseforge_key_input: String::new(),
            pending: HashMap::new(),
            instance_search: String::new(),
            selected_instance: None,
            create: None,
            versions: Vec::new(),
            loader_versions: Vec::new(),
            login: None,
            logs: HashMap::new(),
            mods: HashMap::new(),
            java_candidates: Vec::new(),
            preflight: HashMap::new(),
            core_data_dir: None,
            core_instance_dir: None,
            core_log_dir: None,
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
            modpack_browser: ModpackBrowser::new("curse_forge"),
            modpack: ModpackWizard {
                path: String::new(),
                name: String::new(),
                analysis: None,
                analyzing: false,
                importing: false,
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
        self.pending.clear();
        self.skins.reset_pending();
        self.modpack_icons.reset_pending();
        self.conn = None;
        match CoreConnection::spawn(
            self.preferences.storage_dir.as_deref(),
            self.preferences.log_dir.as_deref(),
        ) {
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
                crate::log::write(&format!("core spawn failed: {key} {detail}"));
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

    pub(crate) fn request_missing_skins(&mut self) {
        let requests: Vec<(String, String)> = self
            .accounts
            .iter()
            .filter_map(|account| {
                let url = account.skin_url.as_deref()?;
                self.skins
                    .needs_request(&account.id, url)
                    .then(|| (account.id.clone(), url.to_owned()))
            })
            .collect();
        for (id, url) in requests {
            self.skins.begin_request(&id, &url);
            self.call("account.skin", json!({"id": id}), json!({"id": id}));
        }
    }

    fn request_missing_modpack_icons(&mut self) {
        let requests: Vec<(String, String)> = self
            .modpack_browser
            .results
            .iter()
            .chain(
                self.installing_modpacks
                    .iter()
                    .map(|install| &install.project),
            )
            .filter_map(|project| {
                let url = project.icon_url.as_deref()?;
                if url.trim().is_empty() {
                    return None;
                }
                let key = modpack_icon_key(project);
                self.modpack_icons
                    .needs_request(&key, url)
                    .then(|| (key, url.to_owned()))
            })
            .collect();
        for (key, url) in requests {
            self.modpack_icons.begin_request(&key, &url);
            self.call("modpack.icon", json!({"url": url}), json!({"id": key}));
        }
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
                    crate::log::write(&format!("core disconnected: {key} {detail}"));
                    let reason = self.trf(&key, &[("detail", &detail)]);
                    self.fatal = Some(self.trf("error.disconnected_stale", &[("reason", &reason)]));
                    self.conn = None;
                    self.pending.clear();
                    break;
                }
            }
        }
    }

    fn modpack_request_matches(&self, pending: &Pending) -> bool {
        pending.context.get("provider").and_then(Value::as_str)
            == Some(self.modpack_browser.provider.as_str())
            && pending.context.get("generation").and_then(Value::as_u64)
                == Some(self.modpack_browser.generation)
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
            "modpack.search" => {
                if self.modpack_request_matches(pending) {
                    self.modpack_browser.searching = false;
                }
            }
            "modpack.analyze" => self.modpack.analyzing = false,
            "modpack.import" => self.modpack.importing = false,
            "modpack.install" => {
                if let Some(project) = pending.context.get("projectId").and_then(Value::as_str) {
                    self.modpack_browser.requested.remove(project);
                    self.installing_modpacks
                        .retain(|install| install.project.project_id != project);
                }
            }
            "instance.create" => {
                if let Some(create) = &mut self.create {
                    create.creating = false;
                }
            }
            "account.skin" => {
                if let Some(id) = pending.context.get("id").and_then(Value::as_str) {
                    self.skins.fail(id);
                }
            }
            "modpack.icon" => {
                if let Some(key) = pending.context.get("id").and_then(Value::as_str) {
                    self.modpack_icons.fail(key);
                }
            }
            _ => {}
        }
        crate::log::write(&format!("{} failed: {message} ({code})", pending.method));
        if !matches!(pending.method.as_str(), "account.skin" | "modpack.icon") {
            self.notice =
                Some(self.trf("notice.request_failed_log", &[("method", &pending.method)]));
        }
    }

    fn handle_event(&mut self, event: Value) {
        let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
        let data = event.get("data").cloned().unwrap_or(Value::Null);
        match kind {
            "job" => {
                if let Ok(job) =
                    serde_json::from_value::<Job>(data.get("job").cloned().unwrap_or(Value::Null))
                {
                    let previous_instance_id = self
                        .jobs
                        .iter()
                        .find(|existing| existing.id == job.id)
                        .and_then(Job::instance_id)
                        .map(str::to_owned);
                    let instance_id = job.instance_id().map(str::to_owned);
                    let terminal = matches!(
                        job.status.as_str(),
                        "succeeded" | "failed" | "cancelled" | "interrupted"
                    );
                    if terminal {
                        if matches!(job.status.as_str(), "succeeded" | "cancelled") {
                            self.installing_modpacks.retain(|install| {
                                install.job_id.as_deref() != Some(job.id.as_str())
                            });
                        }
                        if let Some(project) = self.modpack_browser.install_jobs.remove(&job.id) {
                            self.modpack_browser.requested.remove(&project);
                        }
                        if job.kind == "import" {
                            self.modpack.importing = false;
                            if job.status == "succeeded" {
                                self.selected_instance = job
                                    .result
                                    .as_ref()
                                    .and_then(|result| result.get("instanceId"))
                                    .and_then(Value::as_str)
                                    .map(str::to_owned);
                                self.modpack.analysis = None;
                                self.create = None;
                            } else if job.status == "failed" {
                                let message = job
                                    .error
                                    .as_ref()
                                    .map(|error| error.message.as_str())
                                    .unwrap_or("unknown error");
                                self.notice =
                                    Some(self.trf("notice.import_failed", &[("message", message)]));
                            }
                        }
                        if matches!(job.kind.as_str(), "import" | "install") {
                            self.refresh_instances();
                        }
                    } else if job.kind == "import"
                        && instance_id.is_some()
                        && previous_instance_id != instance_id
                    {
                        self.refresh_instances();
                    }
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
                self.call("system.diagnostics", Value::Null, Value::Null);
            }
            "system.diagnostics" => {
                let extra = result.get("extra").cloned().unwrap_or(Value::Null);
                self.core_data_dir = extra
                    .get("dataDirectory")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                self.core_instance_dir = extra
                    .get("instanceDirectory")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                self.core_log_dir = extra
                    .get("logDirectory")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            "account.list" => {
                self.accounts = parse_list(result);
                let ids: Vec<String> = self
                    .accounts
                    .iter()
                    .map(|account| account.id.clone())
                    .collect();
                self.skins.retain_accounts(&ids);
                self.request_missing_skins();
            }
            "account.skin" => {
                if let Some(id) = pending.context.get("id").and_then(Value::as_str) {
                    let png = result
                        .get("pngBase64")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let model = result
                        .get("model")
                        .and_then(Value::as_str)
                        .unwrap_or("classic");
                    if let Err(error) = self.skins.complete(id, png, model) {
                        crate::log::write(&format!("skin decode failed for {id}: {error}"));
                        self.skins.fail(id);
                    }
                }
            }
            "instance.list" => {
                self.instances = parse_list(result);
                if self.browser.target.is_none() {
                    self.browser.target = self.instances.first().map(|item| item.id.clone());
                }
            }
            "job.list" => {
                self.jobs = parse_list(result);
                self.jobs.reverse();
                let mut installs = Vec::new();
                for install in self
                    .jobs
                    .iter()
                    .rev()
                    .filter_map(InstallingModpack::from_job)
                {
                    if !installs.iter().any(|existing: &InstallingModpack| {
                        existing.project.provider == install.project.provider
                            && existing.project.project_id == install.project.project_id
                    }) {
                        installs.push(install);
                    }
                }
                self.installing_modpacks = installs;
                self.request_missing_modpack_icons();
            }
            "settings.get" | "settings.update" | "settings.reset" => {
                if let Ok(settings) = serde_json::from_value::<AppSettings>(result) {
                    self.settings = Some(settings.clone());
                    self.settings_draft = Some(settings);
                }
            }
            "version.list" => {
                let current = self.create.as_ref().map(|create| create.include_snapshots);
                let requested = pending
                    .context
                    .get("includeSnapshots")
                    .and_then(Value::as_bool);
                if requested.is_none() || requested == current {
                    self.versions = parse_list(result);
                    if let Some(create) = &mut self.create {
                        if create.game_version.is_empty() {
                            create.game_version = self
                                .versions
                                .iter()
                                .find(|version| version.release_type == "release")
                                .or_else(|| self.versions.first())
                                .map(|version| version.id.clone())
                                .unwrap_or_default();
                        }
                    }
                }
            }
            "loader.list" => {
                let matches = self.create.as_ref().is_none_or(|create| {
                    pending.context.get("gameVersion").and_then(Value::as_str)
                        == Some(create.game_version.as_str())
                        && pending.context.get("loader").and_then(Value::as_str)
                            == Some(create.loader.as_str())
                });
                if matches {
                    self.loader_versions = parse_list(result);
                    if let Some(create) = &mut self.create {
                        if create.loader != "vanilla" && create.loader_version.is_empty() {
                            create.loader_version = self
                                .loader_versions
                                .iter()
                                .find(|version| version.stable)
                                .or_else(|| self.loader_versions.first())
                                .map(|version| version.version.clone())
                                .unwrap_or_default();
                        }
                    }
                }
            }
            "instance.create" => match serde_json::from_value::<Instance>(result) {
                Ok(instance) => {
                    self.notice =
                        Some(self.trf("notice.instance_created", &[("name", &instance.name)]));
                    let install = pending
                        .context
                        .get("installAfterCreate")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    self.create = None;
                    if install {
                        self.selected_instance = None;
                        self.call(
                            "instance.install",
                            json!({"id": instance.id}),
                            json!({"instanceId": instance.id}),
                        );
                    } else {
                        self.selected_instance = Some(instance.id);
                    }
                    self.refresh_instances();
                }
                Err(_) => {
                    if let Some(create) = &mut self.create {
                        create.creating = false;
                    }
                }
            },
            "instance.install" | "instance.repair" | "mod.install" | "mod.update"
            | "modpack.import" => {
                if let Ok(job) = serde_json::from_value::<Job>(result) {
                    self.merge_job(job);
                }
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
                    if session.status == "failed" {
                        crate::log::write(&format!(
                            "microsoft login failed: {}",
                            session.error.as_deref().unwrap_or("unknown error")
                        ));
                    }
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
            "modpack.search" => {
                if self.modpack_request_matches(pending) {
                    let mut results: Vec<ModProject> = parse_list(result);
                    self.modpack_browser.has_next = results.len() > MODPACK_PAGE_SIZE;
                    results.truncate(MODPACK_PAGE_SIZE);
                    self.modpack_browser.results = results;
                    self.modpack_browser.searching = false;
                    self.request_missing_modpack_icons();
                }
            }
            "modpack.icon" => {
                if let Some(key) = pending.context.get("id").and_then(Value::as_str) {
                    let data = result.as_str().unwrap_or_default();
                    if let Err(error) = self.modpack_icons.complete(key, data) {
                        crate::log::write(&format!(
                            "modpack icon decode failed for {key}: {error}"
                        ));
                        self.modpack_icons.fail(key);
                    }
                }
            }
            "modpack.install" => {
                if let Ok(job) = serde_json::from_value::<Job>(result) {
                    let terminal = matches!(
                        job.status.as_str(),
                        "succeeded" | "failed" | "cancelled" | "interrupted"
                    );
                    if let Some(project) = pending.context.get("projectId").and_then(Value::as_str)
                    {
                        self.modpack_browser
                            .install_jobs
                            .insert(job.id.clone(), project.to_owned());
                        if let Some(install) = self.installing_modpacks.iter_mut().find(|install| {
                            install.project.project_id == project && install.job_id.is_none()
                        }) {
                            install.job_id = Some(job.id.clone());
                        }
                        if terminal {
                            self.modpack_browser.requested.remove(project);
                            self.modpack_browser.install_jobs.remove(&job.id);
                            if matches!(job.status.as_str(), "succeeded" | "cancelled") {
                                self.installing_modpacks.retain(|install| {
                                    install.job_id.as_deref() != Some(job.id.as_str())
                                });
                            }
                        }
                    }
                    self.merge_job(job);
                } else if let Some(project) =
                    pending.context.get("projectId").and_then(Value::as_str)
                {
                    self.modpack_browser.requested.remove(project);
                    self.installing_modpacks
                        .retain(|install| install.project.project_id != project);
                }
            }
            "modpack.analyze" => {
                self.modpack.analyzing = false;
                self.modpack.analysis = serde_json::from_value(result).ok();
                if self.modpack.name.is_empty() {
                    self.modpack.name = self
                        .modpack
                        .analysis
                        .as_ref()
                        .map(|analysis| analysis.name.clone())
                        .unwrap_or_default();
                }
            }
            _ => {}
        }
    }
}

fn parse_list<T: serde::de::DeserializeOwned>(value: Value) -> Vec<T> {
    serde_json::from_value(value).unwrap_or_default()
}

const NAV_WIDTH: f32 = 168.0;
const NAV_ITEM_HEIGHT: f32 = 38.0;
const NAV_ACCOUNT_HEIGHT: f32 = 58.0;

impl eframe::App for MzApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let theme = (self.preferences.theme, self.system_dark);
        if self.applied_theme != Some(theme) {
            crate::apply_theme(ctx, self.preferences.theme, self.system_dark);
            self.applied_theme = Some(theme);
        }
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
            .exact_width(NAV_WIDTH)
            .show(ctx, |ui| {
                ui.add_space(crate::layout::SECTION_GAP);
                ui.vertical_centered(|ui| {
                    ui.heading(egui::RichText::new("MzLauncher").size(22.0));
                });
                ui.add_space(16.0);
                for (page, key) in [
                    (Page::Instances, "nav.instances"),
                    (Page::Accounts, "nav.accounts"),
                    (Page::Settings, "nav.settings"),
                ] {
                    let selected = self.page == page;
                    if ui
                        .add_sized(
                            [ui.available_width(), NAV_ITEM_HEIGHT],
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
                let account = self
                    .settings
                    .as_ref()
                    .and_then(|settings| settings.default_account_id.as_deref())
                    .and_then(|id| self.accounts.iter().find(|account| account.id == id));
                let (title, subtitle) = match account {
                    Some(account) => (
                        account.name.clone(),
                        self.tr(crate::pages::home::account_kind_key(&account.kind)),
                    ),
                    None => (
                        self.tr("home.no_account"),
                        self.tr("accounts.add_account_short"),
                    ),
                };
                let active = self
                    .jobs
                    .iter()
                    .filter(|job| matches!(job.status.as_str(), "queued" | "running"))
                    .count();
                let mut status = if let Some(handshake) = &self.handshake {
                    self.trf(
                        "status.core_protocol",
                        &[
                            ("version", &handshake.core_version),
                            ("protocol", &handshake.protocol_version.to_string()),
                        ],
                    )
                } else {
                    self.tr("status.core_offline")
                };
                if active > 0 {
                    status.push_str(&format!(
                        " · {}",
                        self.trf("status.active_jobs", &[("count", &active.to_string())])
                    ));
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(crate::layout::SECTION_GAP);
                    let label = egui::RichText::new(format!("{title}\n{subtitle}")).size(13.0);
                    if ui
                        .add_sized(
                            [ui.available_width(), NAV_ACCOUNT_HEIGHT],
                            egui::Button::new(label).frame(true),
                        )
                        .clicked()
                    {
                        self.page = Page::Accounts;
                    }
                    ui.add_space(crate::layout::CARD_LIST_GAP);
                    ui.add(egui::Label::new(egui::RichText::new(status).size(10.0)).truncate());
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
        self.new_instance_window(ctx);
    }
}

pub fn modpack_icon_key(project: &ModProject) -> String {
    format!("{}:{}", project.provider, project.project_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_interrupted_remote_modpack_from_checkpoint() {
        let job = Job {
            id: "job-1".into(),
            kind: "import".into(),
            status: "interrupted".into(),
            target: None,
            progress: JobProgress {
                stage: "downloading".into(),
                completed: 10,
                total: Some(20),
                current_item: None,
            },
            error: None,
            result: None,
            checkpoint: Some(json!({
                "modpack": {
                    "provider": "curse_forge",
                    "projectId": "925200",
                    "projectName": "All the Mods 10",
                    "projectIconUrl": "https://cdn.example.com/atm10.gif",
                    "gameVersion": "1.21.1",
                    "loader": "neo_forge"
                }
            })),
        };
        let mut failed = job.clone();
        failed.status = "failed".into();
        assert!(InstallingModpack::from_job(&failed).is_some());
        let install = InstallingModpack::from_job(&job).unwrap();
        assert_eq!(install.job_id.as_deref(), Some("job-1"));
        assert_eq!(install.project.project_id, "925200");
        assert_eq!(install.game_version, "1.21.1");
        assert_eq!(install.loader, "neo_forge");
        assert_eq!(
            install.project.icon_url.as_deref(),
            Some("https://cdn.example.com/atm10.gif")
        );
    }
}
