use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::config::ConfigManager;
use crate::credentials::CredentialStore;
use crate::error::{CoreError, CoreResult};
use crate::model::{Account, AccountKind};
use crate::store::Store;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginSession {
    pub id: Uuid,
    pub status: LoginStatus,
    pub authorization_url: String,
    pub account_id: Option<Uuid>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginStatus {
    Waiting,
    Completed,
    Failed,
    Cancelled,
}

struct LoginState {
    snapshot: LoginSession,
    cancellation: CancellationToken,
}

#[derive(Debug, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase")]
struct TokenSet {
    microsoft_access_token: String,
    refresh_token: Option<String>,
    minecraft_access_token: String,
    #[zeroize(skip)]
    microsoft_expires_at: DateTime<Utc>,
    #[zeroize(skip)]
    minecraft_expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct XboxResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: XboxClaims,
}

#[derive(Debug, Deserialize)]
struct XboxClaims {
    xui: Vec<XboxUser>,
}

#[derive(Debug, Deserialize)]
struct XboxUser {
    uhs: String,
}

#[derive(Debug, Deserialize)]
struct MinecraftTokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct MinecraftProfile {
    id: String,
    name: String,
}

#[derive(Clone)]
pub struct AuthService {
    config: Arc<ConfigManager>,
    store: Arc<Store>,
    credentials: CredentialStore,
    client: reqwest::Client,
    sessions: Arc<Mutex<HashMap<Uuid, LoginState>>>,
}

impl AuthService {
    pub fn new(config: Arc<ConfigManager>, store: Arc<Store>) -> CoreResult<Self> {
        let current = config.get();
        let mut builder = reqwest::Client::builder()
            .user_agent(format!("MzLauncher-Core/{}", crate::CORE_VERSION))
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(60));
        if let Some(proxy) = &current.download.proxy_url {
            builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|error| {
                CoreError::Configuration(format!("invalid proxy configuration: {error}"))
            })?);
        }
        Ok(Self {
            config,
            store,
            credentials: CredentialStore::new(),
            client: builder
                .build()
                .map_err(|error| CoreError::Configuration(error.to_string()))?,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn list(&self) -> CoreResult<Vec<Account>> {
        self.store.accounts()
    }

    pub async fn begin_login(&self) -> CoreResult<LoginSession> {
        let client_id = self.client_id()?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let redirect_uri = format!("http://{address}/callback");
        let verifier = random_url_safe(48);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let state = random_url_safe(32);
        let id = Uuid::new_v4();
        let authorization_url = format!(
            "{}/authorize?client_id={}&response_type=code&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256&prompt=select_account",
            self.config.get().endpoints.microsoft_authority.trim_end_matches('/'),
            urlencoding(&client_id),
            urlencoding(&redirect_uri),
            urlencoding("XboxLive.signin offline_access"),
            urlencoding(&state),
            urlencoding(&challenge),
        );
        let snapshot = LoginSession {
            id,
            status: LoginStatus::Waiting,
            authorization_url: authorization_url.clone(),
            account_id: None,
            error: None,
        };
        let cancellation = CancellationToken::new();
        self.sessions.lock().await.insert(
            id,
            LoginState {
                snapshot: snapshot.clone(),
                cancellation: cancellation.clone(),
            },
        );
        let service = self.clone();
        tokio::spawn(async move {
            let result = service
                .wait_for_callback(listener, &redirect_uri, &verifier, &state, &cancellation)
                .await;
            let mut sessions = service.sessions.lock().await;
            if let Some(session) = sessions.get_mut(&id) {
                match result {
                    Ok(account_id) => {
                        session.snapshot.status = LoginStatus::Completed;
                        session.snapshot.account_id = Some(account_id);
                    }
                    Err(error) => {
                        session.snapshot.status = if error.is_cancelled() {
                            LoginStatus::Cancelled
                        } else {
                            LoginStatus::Failed
                        };
                        session.snapshot.error = Some(error.to_string());
                    }
                }
            }
        });
        open::that_detached(&authorization_url).map_err(|error| {
            CoreError::Authentication(format!("could not open the system browser: {error}"))
        })?;
        Ok(snapshot)
    }

    pub async fn login_status(&self, id: Uuid) -> CoreResult<LoginSession> {
        self.sessions
            .lock()
            .await
            .get(&id)
            .map(|session| session.snapshot.clone())
            .ok_or_else(|| CoreError::NotFound(format!("login session {id}")))
    }

    pub async fn cancel_login(&self, id: Uuid) -> CoreResult<LoginSession> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(&id)
            .ok_or_else(|| CoreError::NotFound(format!("login session {id}")))?;
        session.cancellation.cancel();
        Ok(session.snapshot.clone())
    }

    async fn wait_for_callback(
        &self,
        listener: TcpListener,
        redirect_uri: &str,
        verifier: &str,
        expected_state: &str,
        cancellation: &CancellationToken,
    ) -> CoreResult<Uuid> {
        let (mut socket, _) = tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(600), listener.accept()) => {
                result.map_err(|_| CoreError::Authentication("login callback timed out".into()))??
            }
            _ = cancellation.cancelled() => return Err(CoreError::Cancelled),
        };
        let callback = read_callback(&mut socket).await?;
        let url = Url::parse(&format!("http://127.0.0.1{callback}"))
            .map_err(|_| CoreError::Authentication("invalid local login callback".into()))?;
        if url.path() != "/callback" {
            return Err(CoreError::Authentication(
                "unexpected login callback path".into(),
            ));
        }
        let parameters = url.query_pairs().collect::<HashMap<_, _>>();
        if parameters.get("state").map(|value| value.as_ref()) != Some(expected_state) {
            return Err(CoreError::Authentication(
                "login callback state does not match".into(),
            ));
        }
        if let Some(error) = parameters.get("error") {
            return Err(CoreError::Authentication(format!(
                "Microsoft authorization was not completed: {error}"
            )));
        }
        let code = parameters
            .get("code")
            .ok_or_else(|| CoreError::Authentication("login callback has no code".into()))?
            .to_string();
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 51\r\nConnection: close\r\n\r\nAuthentication received. Return to MzLauncher now.")
            .await?;
        let endpoints = self.config.get().endpoints.clone();
        let microsoft = self
            .exchange_microsoft_code(
                &endpoints.microsoft_authority,
                &self.client_id()?,
                &code,
                redirect_uri,
                verifier,
            )
            .await?;
        let profile_and_tokens = self.authenticate_minecraft(microsoft, &endpoints).await?;
        self.save_authenticated_account(profile_and_tokens.0, profile_and_tokens.1)
    }

    async fn exchange_microsoft_code(
        &self,
        authority: &str,
        client_id: &str,
        code: &str,
        redirect_uri: &str,
        verifier: &str,
    ) -> CoreResult<TokenResponse> {
        self.microsoft_token(
            authority,
            &[
                ("client_id", client_id),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("code_verifier", verifier),
                ("scope", "XboxLive.signin offline_access"),
            ],
        )
        .await
    }

    async fn microsoft_token(
        &self,
        authority: &str,
        parameters: &[(&str, &str)],
    ) -> CoreResult<TokenResponse> {
        let response = self
            .client
            .post(format!("{}/token", authority.trim_end_matches('/')))
            .form(parameters)
            .send()
            .await
            .map_err(auth_network_error)?;
        parse_response(response, "microsoft").await
    }

    async fn authenticate_minecraft(
        &self,
        microsoft: TokenResponse,
        endpoints: &crate::config::EndpointConfig,
    ) -> CoreResult<(MinecraftProfile, TokenSet)> {
        let xbox: XboxResponse = parse_response(
            self.client
                .post(&endpoints.xbox_auth)
                .json(&json!({
                    "Properties": {
                        "AuthMethod": "RPS",
                        "SiteName": "user.auth.xboxlive.com",
                        "RpsTicket": format!("d={}", microsoft.access_token),
                    },
                    "RelyingParty": "http://auth.xboxlive.com",
                    "TokenType": "JWT",
                }))
                .send()
                .await
                .map_err(auth_network_error)?,
            "xbox_live",
        )
        .await?;
        let user_hash = xbox_user_hash(&xbox)?;
        let xsts: XboxResponse = parse_response(
            self.client
                .post(&endpoints.xsts_auth)
                .json(&json!({
                    "Properties": {
                        "SandboxId": "RETAIL",
                        "UserTokens": [xbox.token],
                    },
                    "RelyingParty": "rp://api.minecraftservices.com/",
                    "TokenType": "JWT",
                }))
                .send()
                .await
                .map_err(auth_network_error)?,
            "xsts",
        )
        .await?;
        let minecraft: MinecraftTokenResponse = parse_response(
            self.client
                .post(&endpoints.minecraft_auth)
                .json(&json!({
                    "identityToken": format!("XBL3.0 x={};{}", user_hash, xsts.token),
                }))
                .send()
                .await
                .map_err(auth_network_error)?,
            "minecraft",
        )
        .await?;
        let profile: MinecraftProfile = parse_response(
            self.client
                .get(&endpoints.minecraft_profile)
                .bearer_auth(&minecraft.access_token)
                .send()
                .await
                .map_err(auth_network_error)?,
            "minecraft_profile",
        )
        .await?;
        let now = Utc::now();
        Ok((
            profile,
            TokenSet {
                microsoft_access_token: microsoft.access_token,
                refresh_token: microsoft.refresh_token,
                minecraft_access_token: minecraft.access_token,
                microsoft_expires_at: now + chrono::Duration::seconds(microsoft.expires_in as i64),
                minecraft_expires_at: now + chrono::Duration::seconds(minecraft.expires_in as i64),
            },
        ))
    }

    fn save_authenticated_account(
        &self,
        profile: MinecraftProfile,
        tokens: TokenSet,
    ) -> CoreResult<Uuid> {
        if let Some(existing) = self.store.accounts()?.into_iter().find(|account| {
            account.profile_id == profile.id && account.kind == AccountKind::Microsoft
        }) {
            let key = existing
                .credential_key
                .clone()
                .ok_or_else(|| CoreError::Storage("account has no credential key".into()))?;
            self.credentials.store_json(&key, &tokens)?;
            let mut account = existing;
            account.name = profile.name;
            account.verified = true;
            account.last_validated_at = Some(Utc::now());
            self.store.save_account(&account)?;
            return Ok(account.id);
        }
        let id = Uuid::new_v4();
        let credential_key = format!("account-{id}");
        self.credentials.store_json(&credential_key, &tokens)?;
        let account = Account {
            id,
            profile_id: profile.id,
            name: profile.name,
            kind: AccountKind::Microsoft,
            verified: true,
            credential_key: Some(credential_key),
            last_validated_at: Some(Utc::now()),
        };
        self.store.save_account(&account)?;
        Ok(id)
    }

    pub async fn access_token(&self, id: Uuid) -> CoreResult<String> {
        let account = self.store.account(id)?;
        if account.kind == AccountKind::Debug {
            return Err(CoreError::Authentication(
                "debug accounts do not have Minecraft access tokens".into(),
            ));
        }
        let key = account.credential_key.ok_or(CoreError::Unauthorized)?;
        let tokens: TokenSet = self.credentials.get_json(&key)?;
        if tokens.minecraft_expires_at > Utc::now() + chrono::Duration::seconds(60) {
            return Ok(tokens.minecraft_access_token.clone());
        }
        let refreshed = self.refresh_tokens(tokens).await?;
        self.credentials.store_json(&key, &refreshed)?;
        Ok(refreshed.minecraft_access_token.clone())
    }

    async fn refresh_tokens(&self, mut tokens: TokenSet) -> CoreResult<TokenSet> {
        let refresh_token = tokens
            .refresh_token
            .clone()
            .ok_or(CoreError::Unauthorized)?;
        let config = self.config.get();
        let microsoft = self
            .microsoft_token(
                &config.endpoints.microsoft_authority,
                &[
                    ("client_id", self.client_id()?.as_str()),
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh_token.as_str()),
                    ("scope", "XboxLive.signin offline_access"),
                ],
            )
            .await?;
        let (_, refreshed) = self
            .authenticate_minecraft(microsoft, &config.endpoints)
            .await?;
        tokens.zeroize();
        Ok(refreshed)
    }

    pub async fn refresh(&self, id: Uuid) -> CoreResult<Account> {
        let account = self.store.account(id)?;
        if account.kind == AccountKind::Debug {
            return Ok(account);
        }
        self.access_token(id).await?;
        let mut account = account;
        account.verified = true;
        account.last_validated_at = Some(Utc::now());
        self.store.save_account(&account)?;
        Ok(account)
    }

    pub fn select(&self, id: Uuid) -> CoreResult<Account> {
        let account = self.store.account(id)?;
        let mut config = self.config.get();
        config.default_account_id = Some(id);
        self.config.replace(config)?;
        Ok(account)
    }

    pub fn logout(&self, id: Uuid) -> CoreResult<()> {
        let account = self.store.account(id)?;
        if let Some(key) = account.credential_key {
            self.credentials.delete(&key)?;
        }
        self.store.delete_account(id)?;
        let mut config = self.config.get();
        if config.default_account_id == Some(id) {
            config.default_account_id = None;
            self.config.replace(config)?;
        }
        Ok(())
    }

    #[cfg(feature = "debug")]
    pub fn debug_login(&self, name: String) -> CoreResult<Account> {
        validate_debug_name(&name)?;
        if let Some(existing) = self
            .store
            .accounts()?
            .into_iter()
            .find(|account| account.kind == AccountKind::Debug && account.name == name)
        {
            return Ok(existing);
        }
        let digest = Sha256::digest(format!("MzLauncher debug:{name}").as_bytes());
        Account {
            id: Uuid::new_v4(),
            profile_id: hex::encode(&digest[..16]),
            name,
            kind: AccountKind::Debug,
            verified: false,
            credential_key: None,
            last_validated_at: None,
        }
        .into_ok(&self.store)
    }

    #[cfg(not(feature = "debug"))]
    pub fn debug_login(&self, _name: String) -> CoreResult<Account> {
        Err(CoreError::Forbidden(
            "debug accounts require a binary built with the debug feature".into(),
        ))
    }

    fn client_id(&self) -> CoreResult<String> {
        self.config
            .get()
            .microsoft_client_id
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                std::env::var("MZLAUNCHER_MICROSOFT_CLIENT_ID")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
            .or_else(|| option_env!("MZLAUNCHER_DEFAULT_CLIENT_ID").map(str::to_owned))
            .ok_or_else(|| {
                CoreError::Configuration(
                    "microsoftClientId must be configured before Microsoft login".into(),
                )
            })
    }
}

#[cfg(feature = "debug")]
trait DebugAccountResult {
    fn into_ok(self, store: &Store) -> CoreResult<Account>;
}

#[cfg(feature = "debug")]
impl DebugAccountResult for Account {
    fn into_ok(self, store: &Store) -> CoreResult<Account> {
        store.save_account(&self)?;
        Ok(self)
    }
}

#[cfg(feature = "debug")]
fn validate_debug_name(name: &str) -> CoreResult<()> {
    if name.trim().is_empty()
        || name.chars().count() > 16
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(CoreError::InvalidInput(
            "debug profile name must contain 1 to 16 ASCII letters, digits, or underscores".into(),
        ));
    }
    Ok(())
}

async fn parse_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    provider: &str,
) -> CoreResult<T> {
    let status = response.status();
    if !status.is_success() {
        let message = match (provider, status.as_u16()) {
            ("minecraft_profile", 404) => "this account does not have a Minecraft profile".into(),
            ("xsts", 401) => {
                "Xbox authorization was rejected; check the account and Xbox profile".into()
            }
            _ => format!("authentication service returned HTTP {}", status.as_u16()),
        };
        return Err(CoreError::Authentication(message));
    }
    response.json().await.map_err(|_| {
        CoreError::Authentication(format!(
            "{provider} returned invalid authentication metadata"
        ))
    })
}

fn xbox_user_hash(response: &XboxResponse) -> CoreResult<String> {
    response
        .display_claims
        .xui
        .first()
        .map(|user| user.uhs.clone())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CoreError::Authentication("Xbox response has no user hash".into()))
}

fn auth_network_error(error: reqwest::Error) -> CoreError {
    CoreError::Network(error.without_url().to_string())
}

fn random_url_safe(size: usize) -> String {
    let mut bytes = vec![0_u8; size];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn urlencoding(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, percent_encoding::NON_ALPHANUMERIC).to_string()
}

async fn read_callback(socket: &mut TcpStream) -> CoreResult<String> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    while bytes.len() < 16 * 1024 {
        let read = tokio::time::timeout(Duration::from_secs(15), socket.read(&mut buffer))
            .await
            .map_err(|_| CoreError::Authentication("login callback read timed out".into()))??;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.windows(2).any(|window| window == b"\r\n") {
            break;
        }
    }
    let request = String::from_utf8(bytes)
        .map_err(|_| CoreError::Authentication("login callback is not valid HTTP".into()))?;
    let line = request.lines().next().unwrap_or_default();
    let mut parts = line.split_whitespace();
    if parts.next() != Some("GET") {
        return Err(CoreError::Authentication(
            "login callback must use GET".into(),
        ));
    }
    parts
        .next()
        .map(ToOwned::to_owned)
        .ok_or_else(|| CoreError::Authentication("login callback has no target".into()))
}

pub fn redacted_launch_argument(argument: &str) -> &str {
    if argument.len() >= 24 && !argument.contains([' ', '/', '\\']) {
        "<redacted>"
    } else {
        argument
    }
}

pub fn account_as_json(account: &Account) -> CoreResult<Value> {
    serde_json::to_value(account).map_err(CoreError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_values_are_url_safe() {
        let verifier = random_url_safe(48);
        assert!(verifier.len() >= 43);
        assert!(!verifier.contains(['+', '/', '=']));
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        assert!(!challenge.contains(['+', '/', '=']));
    }

    #[test]
    fn debug_build_exposes_the_expected_capability() {
        assert_eq!(crate::DEBUG_AUTH, cfg!(feature = "debug"));
    }
}
