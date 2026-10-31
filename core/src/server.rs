use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::mpsc;
use tokio_util::codec::{FramedRead, LinesCodec};
use uuid::Uuid;

use crate::auth::AuthService;
use crate::config::{AppConfig, ConfigManager};
use crate::download::Downloader;
use crate::error::{CoreError, CoreResult};
use crate::instance::InstanceService;
use crate::job::JobManager;
use crate::launch::LaunchService;
use crate::model::{
    CreateInstanceParams, DiagnosticInfo, Event, Instance, JobKind, ModFile, ModProvider, Request,
    Response,
};
use crate::modpack::ModpackService;
use crate::mods::{ModSearchParams, ModService};
use crate::paths::AppPaths;
use crate::protocol::{capabilities, handshake, validate_message, MAX_MESSAGE_BYTES};
use crate::store::Store;
use crate::{CORE_VERSION, DATA_VERSION, PROTOCOL_VERSION};

pub struct Server {
    paths: AppPaths,
    config: Arc<ConfigManager>,
    store: Arc<Store>,
    jobs: JobManager,
    instances: InstanceService,
    auth: AuthService,
    launch: LaunchService,
    mods: ModService,
    modpacks: ModpackService,
    runtime_root: Arc<parking_lot::RwLock<PathBuf>>,
}

enum Outgoing {
    Response(Response),
    Event(Event),
}

impl Server {
    pub fn new(data_dir: Option<PathBuf>, log_dir: Option<PathBuf>) -> CoreResult<Self> {
        let paths = AppPaths::discover(data_dir, log_dir)?;
        paths.initialize()?;
        let config = Arc::new(ConfigManager::load(paths.clone())?);
        let store = Arc::new(Store::open(&paths)?);
        let jobs = JobManager::new(store.clone(), config.get().download.max_concurrent)?;
        let downloader = Downloader::new(config.get().download.clone())?;
        let runtime_root = Arc::new(parking_lot::RwLock::new(config.get().runtime_root(&paths)));
        let instances = InstanceService::new(
            paths.clone(),
            store.clone(),
            downloader.clone(),
            config.get().endpoints.clone(),
            runtime_root.clone(),
            config.clone(),
        );
        let auth = AuthService::new(config.clone(), store.clone(), paths.clone())?;
        let launch = LaunchService::new(
            paths.clone(),
            store.clone(),
            config.clone(),
            auth.clone(),
            instances.versions().clone(),
            instances.java().clone(),
            jobs.clone(),
        );
        let mods = ModService::new(
            paths.clone(),
            store.clone(),
            downloader.clone(),
            config.get().endpoints.clone(),
        )?;
        let modpacks = ModpackService::new(
            paths.clone(),
            store.clone(),
            downloader,
            instances.clone(),
            mods.clone(),
            config.get().endpoints.ftb_api,
        )?;
        Ok(Self {
            paths,
            config,
            store,
            jobs,
            instances,
            auth,
            launch,
            mods,
            modpacks,
            runtime_root,
        })
    }

    pub async fn run(self) -> CoreResult<()> {
        let stdin = tokio::io::stdin();
        let mut input = FramedRead::new(stdin, LinesCodec::new_with_max_length(MAX_MESSAGE_BYTES));
        let mut output = BufWriter::new(tokio::io::stdout());
        let (sender, mut receiver) = mpsc::channel::<Outgoing>(256);
        let mut events = self.jobs.subscribe();
        let event_sender = sender.clone();
        let event_task = tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) => {
                        if event_sender.send(Outgoing::Event(event)).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        let mut handshaken = false;
        let mut eof = false;
        let mut shutdown = false;
        while !eof && !shutdown {
            tokio::select! {
                incoming = input.next() => {
                    match incoming {
                        Some(Ok(line)) => {
                            let (response, should_shutdown) = self.handle_line(&line, &mut handshaken).await;
                            if sender.send(Outgoing::Response(response)).await.is_err() {
                                break;
                            }
                            shutdown = should_shutdown;
                        }
                        Some(Err(error)) => {
                            let response = Response::error(
                                String::new(),
                                CoreError::Protocol(format!("invalid input frame: {error}")).protocol_error(),
                            );
                            sender.send(Outgoing::Response(response)).await.map_err(|_| CoreError::Internal("response queue closed".into()))?;
                        }
                        None => eof = true,
                    }
                }
                outgoing = receiver.recv() => {
                    if let Some(outgoing) = outgoing {
                        write_outgoing(&mut output, outgoing).await?;
                    }
                }
            }
        }
        while let Ok(outgoing) = receiver.try_recv() {
            write_outgoing(&mut output, outgoing).await?;
        }
        event_task.abort();

        if !shutdown {
            while self.jobs.list().iter().any(|job| !job.status.is_terminal()) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        Ok(())
    }

    async fn handle_line(&self, line: &str, handshaken: &mut bool) -> (Response, bool) {
        let value = match serde_json::from_str::<Value>(line) {
            Ok(value) => value,
            Err(error) => {
                return (
                    Response::error(String::new(), CoreError::Json(error).protocol_error()),
                    false,
                );
            }
        };
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if let Err(error) = validate_message(&value) {
            return (Response::error(id, error.protocol_error()), false);
        }
        let request = match serde_json::from_value::<Request>(value) {
            Ok(request) => request,
            Err(error) => {
                return (
                    Response::error(id, CoreError::Json(error).protocol_error()),
                    false,
                );
            }
        };
        if !*handshaken
            && !matches!(
                request.method.as_str(),
                "system.handshake" | "system.health"
            )
        {
            return (
                Response::error(
                    request.id,
                    CoreError::Protocol("handshake is required before this request".into())
                        .protocol_error(),
                ),
                false,
            );
        }
        let should_shutdown = request.method == "system.shutdown";
        match self.dispatch(&request).await {
            Ok(result) => {
                if request.method == "system.handshake" {
                    *handshaken = true;
                }
                (Response::success(request.id, result), should_shutdown)
            }
            Err(error) => (Response::error(request.id, error.protocol_error()), false),
        }
    }

    async fn dispatch(&self, request: &Request) -> CoreResult<Value> {
        match request.method.as_str() {
            "system.handshake" => Ok(serde_json::to_value(handshake(parse_params(
                &request.params,
            )?)?)?),
            "system.health" => Ok(json!({
                "status": "ok",
                "coreVersion": CORE_VERSION,
                "protocolVersion": PROTOCOL_VERSION,
                "dataVersion": DATA_VERSION,
            })),
            "system.version" => Ok(json!({
                "coreVersion": CORE_VERSION,
                "protocolVersion": PROTOCOL_VERSION,
                "dataVersion": DATA_VERSION,
            })),
            "system.diagnostics" => {
                let mut extra = BTreeMap::new();
                extra.insert(
                    "dataDirectory".into(),
                    self.paths.root.display().to_string(),
                );
                extra.insert(
                    "instanceDirectory".into(),
                    self.paths.instances.display().to_string(),
                );
                extra.insert("logDirectory".into(), self.paths.logs.display().to_string());
                extra.insert(
                    "instanceCount".into(),
                    self.store.instances()?.len().to_string(),
                );
                Ok(serde_json::to_value(DiagnosticInfo {
                    core_version: CORE_VERSION.into(),
                    protocol_version: PROTOCOL_VERSION,
                    data_version: DATA_VERSION,
                    platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
                    capabilities: capabilities(),
                    extra,
                })?)
            }
            "system.shutdown" => Ok(Value::Null),
            "settings.get" => Ok(serde_json::to_value(self.config.get())?),
            "settings.update" => {
                let config: AppConfig = parse_params(&request.params)?;
                let config = self.config.replace(config)?;
                *self.runtime_root.write() = config.runtime_root(&self.paths);
                Ok(serde_json::to_value(config)?)
            }
            "settings.reset" => {
                let config = self.config.reset()?;
                *self.runtime_root.write() = config.runtime_root(&self.paths);
                Ok(serde_json::to_value(config)?)
            }
            "account.list" => Ok(serde_json::to_value(self.auth.list()?)?),
            "account.current" => {
                let id = self
                    .config
                    .get()
                    .default_account_id
                    .ok_or(CoreError::Unauthorized)?;
                Ok(serde_json::to_value(self.store.account(id)?)?)
            }
            "account.begin_login" => Ok(serde_json::to_value(self.auth.begin_login().await?)?),
            "account.login_status" => {
                let params: AccountRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(
                    self.auth.login_status(params.id).await?,
                )?)
            }
            "account.cancel_login" => {
                let params: AccountRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(
                    self.auth.cancel_login(params.id).await?,
                )?)
            }
            "account.debug_login" => {
                let params: DebugAccountRequest = parse_params(&request.params)?;
                let name = params.name.ok_or_else(|| {
                    CoreError::InvalidInput("debug profile name is required".into())
                })?;
                Ok(serde_json::to_value(self.auth.debug_login(name)?)?)
            }
            "account.select" => {
                let params: AccountRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(self.auth.select(params.id)?)?)
            }
            "account.refresh" => {
                let params: AccountRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(self.auth.refresh(params.id).await?)?)
            }
            "account.skin" => {
                let params: AccountRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(self.auth.skin(params.id).await?)?)
            }
            "account.logout" => {
                let params: AccountRequest = parse_params(&request.params)?;
                self.auth.logout(params.id)?;
                Ok(Value::Null)
            }
            "job.get" => {
                let id: Uuid = parse_params(&request.params)?;
                Ok(serde_json::to_value(self.jobs.get(id)?)?)
            }
            "job.list" => Ok(serde_json::to_value(self.jobs.list())?),
            "job.cancel" => {
                let id: Uuid = parse_params(&request.params)?;
                Ok(serde_json::to_value(self.jobs.cancel(id)?)?)
            }
            "version.list" => {
                let include_snapshots = request
                    .params
                    .get("includeSnapshots")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                Ok(serde_json::to_value(
                    self.instances.versions().list(include_snapshots).await?,
                )?)
            }
            "loader.list" => {
                let params: LoaderRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(
                    self.instances
                        .loaders()
                        .list(&params.game_version, params.loader)
                        .await?,
                )?)
            }
            "java.list" => Ok(serde_json::to_value(
                self.instances.java().discover().await,
            )?),
            "java.validate" => {
                let params: JavaRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(
                    self.instances
                        .java()
                        .validate(Path::new(&params.path))
                        .await?,
                )?)
            }
            "instance.create" => {
                let params: CreateInstanceParams = parse_params(&request.params)?;
                Ok(serde_json::to_value(self.instances.create(params)?)?)
            }
            "instance.list" => Ok(serde_json::to_value(self.instances.list()?)?),
            "instance.get" => {
                let params: InstanceRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(self.instances.get(params.id)?)?)
            }
            "instance.rename" => {
                let params: InstanceRequest = parse_params(&request.params)?;
                let name = params
                    .name
                    .ok_or_else(|| CoreError::InvalidInput("instance name is required".into()))?;
                Ok(serde_json::to_value(
                    self.instances.rename(params.id, name)?,
                )?)
            }
            "instance.copy" => {
                let params: InstanceRequest = parse_params(&request.params)?;
                let name = params
                    .name
                    .ok_or_else(|| CoreError::InvalidInput("instance name is required".into()))?;
                Ok(serde_json::to_value(
                    self.instances.copy(params.id, name).await?,
                )?)
            }
            "instance.update" => {
                let instance: Instance = parse_params(&request.params)?;
                Ok(serde_json::to_value(
                    self.instances.update_settings(instance)?,
                )?)
            }
            "instance.delete" => {
                let params: InstanceRequest = parse_params(&request.params)?;
                if self.launch.status(params.id).await?.is_some_and(|status| {
                    matches!(
                        status.status,
                        crate::model::GameStatus::Starting
                            | crate::model::GameStatus::Running
                            | crate::model::GameStatus::Stopping
                    )
                }) {
                    return Err(CoreError::Conflict(
                        "stop the game before deleting this instance".into(),
                    ));
                }
                self.instances.delete(params.id).await?;
                Ok(Value::Null)
            }
            "instance.validate" => Ok(serde_json::to_value(
                self.instances
                    .validate(parse_params::<InstanceRequest>(&request.params)?.id)?,
            )?),
            "instance.install" | "instance.repair" => {
                let params: InstanceRequest = parse_params(&request.params)?;
                let service = self.instances.clone();
                let job = self.jobs.submit(
                    JobKind::Install,
                    Some(params.id),
                    params.idempotency_key,
                    move |context| async move { service.install(params.id, &context).await },
                )?;
                Ok(serde_json::to_value(job)?)
            }
            "mod.search" => {
                let params: ModSearchParams = parse_params(&request.params)?;
                Ok(serde_json::to_value(self.mods.search(params).await?)?)
            }
            "mod.files" => {
                let params: ModRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(
                    self.mods
                        .files(
                            required(params.provider, "provider")?,
                            required(params.instance_id, "instanceId")?,
                            required(params.project_id, "projectId")?.as_str(),
                        )
                        .await?,
                )?)
            }
            "mod.resolve" => {
                let params: ModRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(
                    self.mods
                        .resolve(
                            required(params.instance_id, "instanceId")?,
                            required(params.file, "file")?,
                        )
                        .await?,
                )?)
            }
            "mod.install" => {
                let params: ModRequest = parse_params(&request.params)?;
                let instance_id = required(params.instance_id, "instanceId")?;
                let file = required(params.file, "file")?;
                let service = self.mods.clone();
                let job = self.jobs.submit(
                    JobKind::ModUpdate,
                    Some(instance_id),
                    params.idempotency_key,
                    move |context| async move { service.install(instance_id, file, &context).await },
                )?;
                Ok(serde_json::to_value(job)?)
            }
            "mod.list" => {
                let params: ModRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(
                    self.mods
                        .list(required(params.instance_id, "instanceId")?)?,
                )?)
            }
            "mod.set_enabled" => {
                let params: ModRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(self.mods.set_enabled(
                    required(params.id, "id")?,
                    required(params.enabled, "enabled")?,
                )?)?)
            }
            "mod.remove" => {
                let params: ModRequest = parse_params(&request.params)?;
                self.mods.remove(required(params.id, "id")?)?;
                Ok(Value::Null)
            }
            "mod.update" => {
                let params: ModRequest = parse_params(&request.params)?;
                let instance_id = required(params.instance_id, "instanceId")?;
                let service = self.mods.clone();
                let job = self.jobs.submit(
                    JobKind::ModUpdate,
                    Some(instance_id),
                    params.idempotency_key,
                    move |context| async move { service.update(instance_id, &context).await },
                )?;
                Ok(serde_json::to_value(job)?)
            }
            "provider.curseforge_key.set" => {
                let params: ModRequest = parse_params(&request.params)?;
                self.mods
                    .set_curseforge_key(required(params.key, "key")?.as_str())?;
                Ok(Value::Null)
            }
            "provider.curseforge_key.delete" => {
                self.mods.delete_curseforge_key()?;
                Ok(Value::Null)
            }
            "modpack.analyze" => {
                let params: ModpackRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(
                    self.modpacks.analyze(Path::new(&params.path)).await?,
                )?)
            }
            "modpack.import" => {
                let params: ModpackRequest = parse_params(&request.params)?;
                let service = self.modpacks.clone();
                let path = PathBuf::from(params.path);
                let job = self.jobs.submit(
                    JobKind::Import,
                    None,
                    params.idempotency_key,
                    move |context| async move {
                        service.import(&path, params.name, &context).await
                    },
                )?;
                Ok(serde_json::to_value(job)?)
            }
            "modpack.search" => {
                let params: ModSearchParams = parse_params(&request.params)?;
                Ok(serde_json::to_value(self.modpacks.search(params).await?)?)
            }
            "modpack.icon" => {
                let params: crate::modpack_browser::ModpackIconParams =
                    parse_params(&request.params)?;
                Ok(serde_json::to_value(
                    self.modpacks.icon(&params.url).await?,
                )?)
            }
            "modpack.install" => {
                let params: crate::modpack_browser::ModpackDownloadParams =
                    parse_params(&request.params)?;
                crate::modpack_browser::validate_remote_id(&params.project_id)?;
                let service = self.modpacks.clone();
                let idempotency_key = params.idempotency_key.clone();
                let job = self.jobs.submit(
                    JobKind::Import,
                    None,
                    idempotency_key,
                    move |context| async move {
                        context.checkpoint(serde_json::json!({
                            "stage": "queued",
                            "modpack": {
                                "provider": params.provider,
                                "projectId": &params.project_id,
                                "projectName": &params.project_name,
                                "projectIconUrl": &params.project_icon_url,
                                "gameVersion": &params.game_version,
                                "loader": params.loader,
                            }
                        }))?;
                        service.import_remote(params, &context).await
                    },
                )?;
                Ok(serde_json::to_value(job)?)
            }
            "game.preflight" => {
                let params: GameRequest = parse_params(&request.params)?;
                Ok(self.launch.preflight(params.id).await?)
            }
            "game.launch" => {
                let params: GameRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(self.launch.launch(params.id).await?)?)
            }
            "game.status" => {
                let params: GameRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(self.launch.status(params.id).await?)?)
            }
            "game.stop" => {
                let params: GameRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(
                    self.launch.stop(params.id, params.force).await?,
                )?)
            }
            "game.logs" => {
                let params: GameRequest = parse_params(&request.params)?;
                Ok(serde_json::to_value(
                    self.launch
                        .logs(params.id, params.tail.unwrap_or(200))
                        .await?,
                )?)
            }
            method => Err(CoreError::NotFound(format!("method {method}"))),
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstanceRequest {
    id: Uuid,
    name: Option<String>,
    idempotency_key: Option<String>,
}

#[derive(serde::Deserialize)]
struct JavaRequest {
    path: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoaderRequest {
    game_version: String,
    loader: crate::model::LoaderKind,
}

#[derive(serde::Deserialize)]
struct AccountRequest {
    id: Uuid,
}

#[derive(serde::Deserialize)]
struct DebugAccountRequest {
    name: Option<String>,
}

#[derive(serde::Deserialize)]
struct GameRequest {
    id: Uuid,
    #[serde(default)]
    force: bool,
    tail: Option<usize>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModRequest {
    provider: Option<ModProvider>,
    instance_id: Option<Uuid>,
    project_id: Option<String>,
    file: Option<ModFile>,
    id: Option<Uuid>,
    enabled: Option<bool>,
    key: Option<String>,
    idempotency_key: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModpackRequest {
    path: String,
    name: Option<String>,
    idempotency_key: Option<String>,
}

fn required<T>(value: Option<T>, name: &str) -> CoreResult<T> {
    value.ok_or_else(|| CoreError::InvalidInput(format!("{name} is required")))
}

fn parse_params<T: DeserializeOwned>(value: &Value) -> CoreResult<T> {
    serde_json::from_value(value.clone()).map_err(|error| {
        CoreError::InvalidInput(format!(
            "request parameters do not match the method: {error}"
        ))
    })
}

async fn write_outgoing(
    output: &mut BufWriter<tokio::io::Stdout>,
    outgoing: Outgoing,
) -> CoreResult<()> {
    let line = match outgoing {
        Outgoing::Response(response) => serde_json::to_string(&response)?,
        Outgoing::Event(event) => serde_json::to_string(&json!({"event": event}))?,
    };
    output.write_all(line.as_bytes()).await?;
    output.write_all(b"\n").await?;
    output.flush().await?;
    Ok(())
}
