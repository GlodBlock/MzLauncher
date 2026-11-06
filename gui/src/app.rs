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
pub const MOD_PAGE_SIZE: usize = 10;

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
pub enum ConfigurationTab {
    Launch,
    Runtime,
    Modpack,
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

#[derive(Default)]
pub struct InstanceModpackConfiguration {
    pub record: Option<ModpackRecord>,
    pub loaded: bool,
    pub current_version: Option<String>,
    pub versions: Vec<ModFile>,
    pub selected_version: Option<String>,
    pub loading: bool,
    pub confirm_update: Option<String>,
    pub mods_offset: usize,
    pub mods_total: usize,
    pub mods_loading: bool,
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
pub struct ModpackVersionSelection {
    pub project: ModProject,
    pub versions: Vec<ModFile>,
    pub selected: Option<String>,
    pub loading: bool,
    pub changelog: String,
    pub changelog_loading: bool,
}

#[derive(Clone)]
pub struct InstallingModpack {
    pub project: ModProject,
    pub file_id: Option<String>,
    pub game_version: String,
    pub loader: String,
    pub job_id: Option<String>,
    pub instance_id: Option<String>,
}

impl InstallingModpack {
    pub(crate) fn from_job(job: &Job) -> Option<Self> {
        let unfinished = matches!(
            job.status.as_str(),
            "queued" | "running" | "cancelling" | "interrupted" | "failed"
        );
        let incomplete = job.status == "succeeded"
            && job
                .result
                .as_ref()
                .and_then(|result| result.get("complete"))
                .and_then(Value::as_bool)
                == Some(false);
        if job.kind != "import" || !(unfinished || incomplete) {
            return None;
        }
        let source = job.checkpoint.as_ref()?.get("modpack")?;
        Some(Self {
            project: Self::project_from_job(job)?,
            file_id: source
                .get("fileId")
                .and_then(Value::as_str)
                .map(str::to_owned),
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
            instance_id: job.instance_id().map(str::to_owned),
        })
    }

    pub(crate) fn project_from_job(job: &Job) -> Option<ModProject> {
        let source = job.checkpoint.as_ref()?.get("modpack")?;
        Some(ModProject {
            provider: source.get("provider")?.as_str()?.to_owned(),
            project_id: source.get("projectId")?.as_str()?.to_owned(),
            name: source.get("projectName")?.as_str()?.to_owned(),
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

#[derive(Clone)]
pub struct DeleteConfirmation {
    pub instance_id: Option<String>,
    pub job_id: Option<String>,
    pub name: String,
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
    pub configuration_tab: ConfigurationTab,
    pub accounts: Vec<Account>,
    pub instances: Vec<Instance>,
    pub jobs: Vec<Job>,
    pub installing_modpacks: Vec<InstallingModpack>,
    pub launch_after_install: std::collections::HashSet<String>,
    pub games: HashMap<String, String>,
    pub last_game_status_poll: Option<std::time::Instant>,
    pub exit_codes: HashMap<String, i32>,
    pub settings: Option<AppSettings>,
    pub settings_draft: Option<AppSettings>,
    pub curseforge_key_input: String,
    pub pending: HashMap<String, Pending>,
    pub instance_search: String,
    pub selected_instance: Option<String>,
    pub create: Option<NewInstanceState>,
    pub modpack_version_selection: Option<ModpackVersionSelection>,
    pub versions: Vec<VersionSummary>,
    pub loader_versions: Vec<LoaderVersion>,
    pub instance_versions: Vec<VersionSummary>,
    pub instance_loader_versions: Vec<LoaderVersion>,
    pub login: Option<LoginSession>,
    pub logs: HashMap<String, Vec<String>>,
    pub mods: HashMap<String, Vec<InstalledMod>>,
    pub java_candidates: Vec<JavaInfo>,
    pub preflight: HashMap<String, Value>,
    pub core_data_dir: Option<String>,
    pub core_instance_dir: Option<String>,
    pub core_log_dir: Option<String>,
    pub confirm_delete: Option<DeleteConfirmation>,
    pub install_details_job: Option<String>,
    pub browser: ModBrowser,
    pub modpack_browser: ModpackBrowser,
    pub modpack: ModpackWizard,
    pub draft_instance: Option<Instance>,
    pub instance_min_memory_input: String,
    pub instance_max_memory_input: String,
    pub instance_jvm_args_input: String,
    pub instance_modpack: InstanceModpackConfiguration,
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
            configuration_tab: ConfigurationTab::Launch,
            accounts: Vec::new(),
            instances: Vec::new(),
            jobs: Vec::new(),
            installing_modpacks: Vec::new(),
            launch_after_install: std::collections::HashSet::new(),
            games: HashMap::new(),
            last_game_status_poll: None,
            exit_codes: HashMap::new(),
            settings: None,
            settings_draft: None,
            curseforge_key_input: String::new(),
            pending: HashMap::new(),
            instance_search: String::new(),
            selected_instance: None,
            create: None,
            modpack_version_selection: None,
            versions: Vec::new(),
            loader_versions: Vec::new(),
            instance_versions: Vec::new(),
            instance_loader_versions: Vec::new(),
            login: None,
            logs: HashMap::new(),
            mods: HashMap::new(),
            java_candidates: Vec::new(),
            preflight: HashMap::new(),
            core_data_dir: None,
            core_instance_dir: None,
            core_log_dir: None,
            confirm_delete: None,
            install_details_job: None,
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
            instance_min_memory_input: String::new(),
            instance_max_memory_input: String::new(),
            instance_jvm_args_input: String::new(),
            instance_modpack: InstanceModpackConfiguration::default(),
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

    fn poll_game_status(&mut self) {
        let now = std::time::Instant::now();
        if self
            .last_game_status_poll
            .is_some_and(|last| now.duration_since(last) < std::time::Duration::from_millis(500))
        {
            return;
        }
        self.last_game_status_poll = Some(now);
        let mut active = self
            .games
            .iter()
            .filter(|(_, status)| matches!(status.as_str(), "starting" | "running" | "stopping"))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        active.sort();
        for id in active {
            self.call("game.status", json!({"id": id}), json!({"instanceId": id}));
        }
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
        let completed_projects: Vec<ModProject> = self
            .jobs
            .iter()
            .filter(|job| job.kind == "import" && job.status == "succeeded")
            .filter_map(InstallingModpack::project_from_job)
            .collect();
        let requests: Vec<(String, String)> = self
            .modpack_browser
            .results
            .iter()
            .chain(
                self.installing_modpacks
                    .iter()
                    .map(|install| &install.project),
            )
            .chain(completed_projects.iter())
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

    fn prepare_mod_icons(&mut self, mods: &[InstalledMod]) {
        for installed in mods {
            let key = mod_icon_key(installed);
            if let Some(url) = installed.icon_url.as_deref().filter(|url| !url.is_empty()) {
                if self.modpack_icons.needs_request(&key, url) {
                    self.modpack_icons.begin_request(&key, url);
                    self.call("modpack.icon", json!({"url": url}), json!({"id": key}));
                }
            } else if matches!(installed.file.provider.as_str(), "manual" | "modpack")
                && self.modpack_icons.needs_request(&key, "jar")
            {
                self.modpack_icons.begin_request(&key, "jar");
                self.call("mod.icon", json!({"id": installed.id}), json!({"id": key}));
            }
        }
    }

    fn request_missing_mod_metadata(&mut self, mods: &[InstalledMod]) {
        for installed in mods {
            if !matches!(installed.file.provider.as_str(), "modrinth" | "curse_forge")
                || installed.icon_url.is_some()
                || self.pending.values().any(|pending| {
                    pending.method == "mod.metadata"
                        && pending.context.get("modId").and_then(Value::as_str)
                            == Some(installed.id.as_str())
                })
            {
                continue;
            }
            self.call(
                "mod.metadata",
                json!({"id": installed.id}),
                json!({"instanceId": installed.instance_id, "modId": installed.id}),
            );
        }
    }

    pub fn refresh_mods(&mut self, instance_id: &str) {
        let offset = if self.selected_instance.as_deref() == Some(instance_id) {
            self.instance_modpack.mods_offset
        } else {
            0
        };
        self.refresh_mods_page(instance_id, offset);
    }

    pub fn refresh_mods_page(&mut self, instance_id: &str, offset: usize) {
        self.instance_modpack.mods_offset = offset;
        self.instance_modpack.mods_loading = true;
        self.call(
            "mod.list",
            json!({"instanceId": instance_id, "offset": offset, "limit": MOD_PAGE_SIZE}),
            json!({"instanceId": instance_id, "offset": offset}),
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
            "modpack.versions" => {
                if pending
                    .context
                    .get("configuration")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    if pending.context.get("instanceId").and_then(Value::as_str)
                        == self.selected_instance.as_deref()
                    {
                        self.instance_modpack.loading = false;
                    }
                } else if let Some(selection) = &mut self.modpack_version_selection {
                    if pending.context.get("projectId").and_then(Value::as_str)
                        == Some(selection.project.project_id.as_str())
                    {
                        selection.loading = false;
                    }
                }
            }
            "modpack.get" => {
                if pending.context.get("instanceId").and_then(Value::as_str)
                    == self.selected_instance.as_deref()
                {
                    self.instance_modpack.loaded = true;
                    self.instance_modpack.record = None;
                }
            }
            "modpack.changelog" => {
                if let Some(selection) = &mut self.modpack_version_selection {
                    let matches = pending.context.get("projectId").and_then(Value::as_str)
                        == Some(selection.project.project_id.as_str())
                        && pending.context.get("provider").and_then(Value::as_str)
                            == Some(selection.project.provider.as_str())
                        && pending.context.get("fileId").and_then(Value::as_str)
                            == selection.selected.as_deref();
                    if matches {
                        selection.changelog.clear();
                        selection.changelog_loading = false;
                    }
                }
            }
            "mod.list" => {
                let id = pending.context.get("instanceId").and_then(Value::as_str);
                if id == self.selected_instance.as_deref()
                    && pending.context.get("offset").and_then(Value::as_u64)
                        == Some(self.instance_modpack.mods_offset as u64)
                {
                    self.instance_modpack.mods_loading = false;
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
            "instance.install" => {
                if pending
                    .context
                    .get("launchAfterInstall")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    if let Some(id) = pending.context.get("instanceId").and_then(Value::as_str) {
                        self.launch_after_install.remove(id);
                    }
                }
            }
            "account.skin" => {
                if let Some(id) = pending.context.get("id").and_then(Value::as_str) {
                    self.skins.fail(id);
                }
            }
            "modpack.icon" | "mod.icon" => {
                if let Some(key) = pending.context.get("id").and_then(Value::as_str) {
                    self.modpack_icons.fail(key);
                }
            }
            _ => {}
        }
        crate::log::write(&format!("{} failed: {message} ({code})", pending.method));
        if !matches!(
            pending.method.as_str(),
            "account.skin" | "modpack.icon" | "mod.icon" | "modpack.changelog" | "mod.metadata"
        ) {
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
                            let updating = job
                                .checkpoint
                                .as_ref()
                                .and_then(|checkpoint| checkpoint.get("update"))
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                            if job.status == "succeeded" {
                                let completed_instance = job
                                    .result
                                    .as_ref()
                                    .and_then(|result| result.get("instanceId"))
                                    .and_then(Value::as_str)
                                    .map(str::to_owned);
                                if self.selected_instance == completed_instance {
                                    if let Some(id) = completed_instance {
                                        self.call(
                                            "modpack.get",
                                            json!({"id": id}),
                                            json!({"instanceId": id}),
                                        );
                                        self.call(
                                            "instance.get",
                                            json!({"id": id}),
                                            json!({"configurationId": id}),
                                        );
                                        self.refresh_mods(&id);
                                    }
                                }
                                if !updating {
                                    self.modpack.analysis = None;
                                    self.create = None;
                                }
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
                        if let Some(id) =
                            take_launch_after_install(&mut self.launch_after_install, &job)
                        {
                            self.call("game.launch", json!({"id": id}), json!({"instanceId": id}));
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
                self.installing_modpacks = restored_modpack_installs(&self.jobs);
                self.request_missing_modpack_icons();
            }
            "settings.get" | "settings.update" | "settings.reset" => {
                if let Ok(settings) = serde_json::from_value::<AppSettings>(result) {
                    self.settings = Some(settings.clone());
                    self.settings_draft = Some(settings);
                }
            }
            "version.list" => {
                let configuration = pending
                    .context
                    .get("configurationId")
                    .and_then(Value::as_str);
                let current = self.create.as_ref().map(|create| create.include_snapshots);
                let requested = pending
                    .context
                    .get("includeSnapshots")
                    .and_then(Value::as_bool);
                let matches = if let Some(id) = configuration {
                    self.selected_instance.as_deref() == Some(id)
                } else {
                    requested.is_none() || requested == current
                };
                if matches {
                    if configuration.is_some() {
                        self.instance_versions = parse_list(result);
                    } else {
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
            }
            "loader.list" => {
                let configuration = pending
                    .context
                    .get("configurationId")
                    .and_then(Value::as_str);
                let matches = if let Some(id) = configuration {
                    self.selected_instance.as_deref() == Some(id)
                        && self.draft_instance.as_ref().is_some_and(|draft| {
                            pending.context.get("gameVersion").and_then(Value::as_str)
                                == Some(draft.game_version.as_str())
                                && pending.context.get("loader").and_then(Value::as_str)
                                    == Some(draft.loader.as_str())
                        })
                } else {
                    self.create.as_ref().is_some_and(|create| {
                        pending.context.get("gameVersion").and_then(Value::as_str)
                            == Some(create.game_version.as_str())
                            && pending.context.get("loader").and_then(Value::as_str)
                                == Some(create.loader.as_str())
                    })
                };
                if matches {
                    if configuration.is_some() {
                        self.instance_loader_versions = parse_list(result);
                        let default_version = self
                            .instance_loader_versions
                            .iter()
                            .find(|version| version.stable)
                            .or_else(|| self.instance_loader_versions.first())
                            .map(|version| version.version.clone());
                        let mut save_runtime = false;
                        if let Some(draft) = &mut self.draft_instance {
                            if draft.loader != "vanilla" && draft.loader_version.is_none() {
                                draft.loader_version = default_version;
                                save_runtime = draft.loader_version.is_some();
                            }
                        }
                        if save_runtime {
                            if let Some(draft) = self.draft_instance.clone() {
                                self.save_instance_configuration(draft);
                            }
                        }
                    } else {
                        self.loader_versions = parse_list(result);
                        let default_version = self
                            .loader_versions
                            .iter()
                            .find(|version| version.stable)
                            .or_else(|| self.loader_versions.first())
                            .map(|version| version.version.clone());
                        if let Some(create) = &mut self.create {
                            if create.loader != "vanilla" && create.loader_version.is_empty() {
                                create.loader_version = default_version.unwrap_or_default();
                            }
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
                        self.close_instance_configuration();
                        self.call(
                            "instance.install",
                            json!({"id": instance.id}),
                            json!({"instanceId": instance.id}),
                        );
                    } else {
                        self.instances.push(instance.clone());
                        self.open_instance_configuration(&instance.id);
                    }
                    self.refresh_instances();
                }
                Err(_) => {
                    if let Some(create) = &mut self.create {
                        create.creating = false;
                    }
                }
            },
            "instance.get" => {
                if let Ok(instance) = serde_json::from_value::<Instance>(result) {
                    if self.selected_instance.as_deref() == Some(instance.id.as_str()) {
                        self.draft_instance = Some(instance);
                    }
                }
            }
            "instance.install" | "instance.repair" | "mod.install" | "mod.update"
            | "modpack.import" | "modpack.update" => {
                if let Ok(job) = serde_json::from_value::<Job>(result) {
                    self.merge_job(job);
                }
            }
            "instance.delete" => {
                self.notice = Some(self.tr("notice.instance_deleted"));
                self.close_instance_configuration();
                self.refresh_instances();
            }
            "instance.update" | "instance.reorder" => self.refresh_instances(),
            "instance.copy" | "instance.rename" => {
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
                        self.call("settings.get", Value::Null, Value::Null);
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
                    let matches = self.selected_instance.as_deref() == Some(id)
                        && pending.context.get("offset").and_then(Value::as_u64)
                            == Some(self.instance_modpack.mods_offset as u64);
                    if matches {
                        if let Ok(page) = serde_json::from_value::<ModListPage>(result) {
                            self.instance_modpack.mods_total = page.total;
                            self.instance_modpack.mods_loading = false;
                            self.request_missing_mod_metadata(&page.items);
                            self.prepare_mod_icons(&page.items);
                            self.mods.insert(id.to_owned(), page.items);
                        }
                    }
                }
            }
            "mod.metadata" => {
                if let Ok(updated) = serde_json::from_value::<InstalledMod>(result) {
                    if let Some(mods) = self.mods.get_mut(&updated.instance_id) {
                        if let Some(current) =
                            mods.iter_mut().find(|current| current.id == updated.id)
                        {
                            *current = updated.clone();
                        }
                    }
                    self.prepare_mod_icons(std::slice::from_ref(&updated));
                }
            }
            "mod.set_enabled" | "mod.remove" => {
                if let Some(id) = pending.context.get("instanceId").and_then(Value::as_str) {
                    self.refresh_mods(id);
                }
            }
            "modpack.get" => {
                let id = pending
                    .context
                    .get("instanceId")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if id.as_deref() == self.selected_instance.as_deref() {
                    self.instance_modpack.record = serde_json::from_value(result).ok();
                    self.instance_modpack.current_version = None;
                    self.instance_modpack.versions.clear();
                    self.instance_modpack.selected_version = None;
                    self.instance_modpack.loaded = true;
                    let has_source = self
                        .instance_modpack
                        .record
                        .as_ref()
                        .is_some_and(|record| record.source.is_some());
                    if has_source {
                        if let Some(id) = id {
                            self.request_instance_modpack_versions(&id);
                        }
                    }
                }
            }
            "modpack.versions" => {
                let configuration = pending
                    .context
                    .get("configuration")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if configuration {
                    if pending.context.get("instanceId").and_then(Value::as_str)
                        == self.selected_instance.as_deref()
                    {
                        let current = self
                            .instance_modpack
                            .record
                            .as_ref()
                            .and_then(|record| record.source.as_ref())
                            .map(|source| source.file_id.as_str());
                        let (current_version, versions) =
                            resolve_modpack_versions(parse_list(result), current);
                        self.instance_modpack.current_version = current_version;
                        self.instance_modpack.selected_version = versions
                            .first()
                            .map(|version| version.file_id.clone())
                            .or_else(|| current.map(str::to_owned));
                        self.instance_modpack.versions = versions;
                        self.instance_modpack.loading = false;
                    }
                } else {
                    let mut changelog_request = None;
                    if let Some(selection) = &mut self.modpack_version_selection {
                        let matches = pending.context.get("projectId").and_then(Value::as_str)
                            == Some(selection.project.project_id.as_str())
                            && pending.context.get("provider").and_then(Value::as_str)
                                == Some(selection.project.provider.as_str());
                        if matches {
                            selection.versions = parse_list(result);
                            selection.selected = selection
                                .versions
                                .first()
                                .map(|version| version.file_id.clone());
                            selection.loading = false;
                            changelog_request = selection
                                .selected
                                .clone()
                                .map(|file_id| (selection.project.clone(), file_id));
                        }
                    }
                    if let Some((project, file_id)) = changelog_request {
                        self.request_modpack_changelog(project, file_id);
                    }
                }
            }
            "modpack.changelog" => {
                if let Some(selection) = &mut self.modpack_version_selection {
                    let matches = pending.context.get("projectId").and_then(Value::as_str)
                        == Some(selection.project.project_id.as_str())
                        && pending.context.get("provider").and_then(Value::as_str)
                            == Some(selection.project.provider.as_str())
                        && pending.context.get("fileId").and_then(Value::as_str)
                            == selection.selected.as_deref();
                    if matches {
                        selection.changelog = result.as_str().unwrap_or_default().trim().to_owned();
                        selection.changelog_loading = false;
                    }
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
            "modpack.icon" | "mod.icon" => {
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
                    if pending
                        .context
                        .get("showDetails")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        self.install_details_job = Some(job.id.clone());
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

fn restored_modpack_installs(jobs: &[Job]) -> Vec<InstallingModpack> {
    let mut seen = std::collections::HashSet::new();
    let mut installs = Vec::new();
    for job in jobs.iter().filter(|job| job.kind == "import") {
        let Some(source) = job
            .checkpoint
            .as_ref()
            .and_then(|value| value.get("modpack"))
        else {
            continue;
        };
        let (Some(provider), Some(project_id)) = (
            source.get("provider").and_then(Value::as_str),
            source.get("projectId").and_then(Value::as_str),
        ) else {
            continue;
        };
        if seen.insert((provider.to_owned(), project_id.to_owned())) {
            if let Some(install) = InstallingModpack::from_job(job) {
                installs.push(install);
            }
        }
    }
    installs
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
        self.poll_game_status();
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
        self.instance_configuration_window(ctx);
        self.modpack_version_window(ctx);
        self.install_details_window(ctx);
    }
}

fn take_launch_after_install(
    launches: &mut std::collections::HashSet<String>,
    job: &Job,
) -> Option<String> {
    if job.kind != "install"
        || !matches!(
            job.status.as_str(),
            "succeeded" | "failed" | "cancelled" | "interrupted"
        )
    {
        return None;
    }
    let id = job.instance_id()?.to_owned();
    let requested = launches.remove(&id);
    (requested && job.status == "succeeded").then_some(id)
}

fn resolve_modpack_versions(
    versions: Vec<ModFile>,
    current: Option<&str>,
) -> (Option<String>, Vec<ModFile>) {
    let current_version = versions
        .iter()
        .find(|version| Some(version.file_id.as_str()) == current)
        .map(|version| version.version_name.clone());
    (current_version, versions)
}

pub fn mod_icon_key(installed: &InstalledMod) -> String {
    format!("mod:{}", installed.id)
}

pub fn modpack_icon_key(project: &ModProject) -> String {
    format!("{}:{}", project.provider, project.project_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_modpack_version_preserves_provider_order() {
        let file = |id: &str, version: &str| {
            serde_json::from_value::<ModFile>(json!({
                "provider": "modrinth",
                "projectId": "project",
                "fileId": id,
                "projectName": "Project",
                "versionName": version,
                "fileName": "pack.mrpack",
                "url": null,
                "size": null,
                "sha1": null,
                "sha256": null,
                "gameVersions": [],
                "loaders": [],
                "channel": "release",
                "dependencies": [],
                "publishedAt": null,
                "distributionAllowed": true
            }))
            .unwrap()
        };
        let versions = vec![file("current-id", "1.2.3"), file("next-id", "1.3.0")];
        let (current, versions) = resolve_modpack_versions(versions, Some("current-id"));

        assert_eq!(current.as_deref(), Some("1.2.3"));
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].file_id, "current-id");
        assert_eq!(versions[1].file_id, "next-id");
    }

    #[test]
    fn launches_only_after_a_requested_successful_install() {
        let mut launches = std::collections::HashSet::from(["instance-1".to_owned()]);
        let mut job: Job = serde_json::from_value(json!({
            "id": "job-1",
            "kind": "install",
            "status": "running",
            "target": "instance-1",
            "progress": {"stage": "installing", "completed": 0, "total": null},
            "error": null,
            "result": null,
            "checkpoint": null
        }))
        .unwrap();
        assert_eq!(take_launch_after_install(&mut launches, &job), None);
        assert!(launches.contains("instance-1"));

        job.status = "failed".into();
        assert_eq!(take_launch_after_install(&mut launches, &job), None);
        assert!(launches.is_empty());

        launches.insert("instance-1".into());
        job.status = "succeeded".into();
        assert_eq!(
            take_launch_after_install(&mut launches, &job).as_deref(),
            Some("instance-1")
        );
        assert!(launches.is_empty());
        assert_eq!(take_launch_after_install(&mut launches, &job), None);
    }

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
                active_items: Vec::new(),
                pending_items: Vec::new(),
            },
            error: None,
            result: None,
            checkpoint: Some(json!({
                "instanceId": "instance-1",
                "modpack": {
                    "provider": "curse_forge",
                    "projectId": "925200",
                    "projectName": "All the Mods 10",
                    "projectIconUrl": "https://cdn.example.com/atm10.gif",
                    "fileId": "6000",
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
        assert_eq!(install.instance_id.as_deref(), Some("instance-1"));
        assert_eq!(install.project.project_id, "925200");
        assert_eq!(install.file_id.as_deref(), Some("6000"));
        assert_eq!(install.game_version, "1.21.1");
        assert_eq!(install.loader, "neo_forge");
        assert_eq!(
            install.project.icon_url.as_deref(),
            Some("https://cdn.example.com/atm10.gif")
        );
        let mut succeeded = job.clone();
        succeeded.status = "succeeded".into();
        let project = InstallingModpack::project_from_job(&succeeded).unwrap();
        assert_eq!(
            project.icon_url.as_deref(),
            Some("https://cdn.example.com/atm10.gif")
        );
        assert!(InstallingModpack::from_job(&succeeded).is_none());
        succeeded.result = Some(json!({"complete": false}));
        let retry = InstallingModpack::from_job(&succeeded).unwrap();
        assert_eq!(retry.file_id.as_deref(), Some("6000"));
        assert_eq!(retry.instance_id.as_deref(), Some("instance-1"));
    }

    #[test]
    fn successful_modpack_supersedes_older_interrupted_job() {
        let interrupted: Job = serde_json::from_value(json!({
            "id": "interrupted-job",
            "kind": "import",
            "status": "interrupted",
            "target": null,
            "progress": {
                "stage": "downloading",
                "completed": 10,
                "total": 20,
                "currentItem": null
            },
            "error": null,
            "result": null,
            "checkpoint": {
                "instanceId": "instance-1",
                "modpack": {
                    "provider": "curse_forge",
                    "projectId": "925200",
                    "projectName": "All the Mods 10",
                    "projectIconUrl": "https://cdn.example.com/atm10.gif"
                }
            }
        }))
        .unwrap();
        let mut succeeded = interrupted.clone();
        succeeded.id = "succeeded-job".into();
        succeeded.status = "succeeded".into();

        let installs = restored_modpack_installs(&[succeeded, interrupted]);
        assert!(installs.is_empty());
    }
}
