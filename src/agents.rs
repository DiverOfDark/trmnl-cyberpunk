//! Coding-agent usage for the desk screen: Claude and Codex rate limits,
//! pulled with logins made from the web UI (`/agents`), plus a per-day token
//! history that machines running the agents push in.
//!
//! **Logins.** The server signs in to each subscription the way its CLI does,
//! as an app of its own — a separate login, so it never shares (and never
//! rotates away) a CLI's refresh token:
//! - Claude: OAuth with PKCE and the manual redirect. The user approves in a
//!   new tab, and the callback page shows a code to paste back into `/agents`.
//! - Codex: the device-code flow. `/agents` shows a code to enter at
//!   auth.openai.com, and the server polls until it's approved.
//!
//! Tokens are kept in `$DATA_DIR` in the CLIs' own file formats and refreshed
//! in place when the access token runs out.
//!
//! **Limits.** Both subscriptions expose the numbers their CLIs show on
//! `/usage` / `/status`: a rolling 5-hour session window and a weekly one.
//!
//! **Tokens.** Neither subscription reports tokens per day; the transcripts on
//! the machines running the agents do. `ccusage` reads them, and its
//! `ccusage {claude,codex} daily --json` output is PUT to
//! `/api/agents/{provider}/tokens` as-is (`scripts/push-agent-tokens.sh`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tracing::{info, warn};
use utoipa::ToSchema;

use crate::data::AgentUsage;
use crate::fetch::NotConfigured;

// ── Credential files ─────────────────────────────────────────────────────────

/// Refresh this long before the access token expires, so a request never
/// races the expiry.
const EXPIRY_MARGIN_SECS: i64 = 300;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Provider {
    Claude,
    Codex,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    fn file(self) -> &'static str {
        match self {
            Self::Claude => "claude-credentials.json",
            Self::Codex => "codex-auth.json",
        }
    }
}

/// One login on disk, in its CLI's file format; we only touch the token
/// fields. No file means signed out.
pub struct Login {
    provider: Provider,
    path: PathBuf,
    /// Serializes refreshes: two overlapping ones would spend the same
    /// single-use refresh token, and the loser would log us out.
    lock: Mutex<()>,
}

impl Login {
    fn new(provider: Provider, data_dir: &Path) -> Self {
        Self {
            provider,
            path: data_dir.join(provider.file()),
            lock: Mutex::new(()),
        }
    }

    fn signed_in(&self) -> bool {
        self.path.exists()
    }

    /// The account the login belongs to, for the web UI.
    fn account(&self) -> Option<String> {
        let doc = read_json(&self.path).ok()?;
        match self.provider {
            Provider::Claude => doc["claudeAiOauth"]["account"].as_str().map(str::to_string),
            Provider::Codex => doc["tokens"]["id_token"]
                .as_str()
                .and_then(jwt_claims)
                .and_then(|c| c["email"].as_str().map(str::to_string)),
        }
    }

    async fn store(&self, doc: &Value) -> Result<()> {
        let _guard = self.lock.lock().await;
        write_json(&self.path, doc)
    }

    async fn forget(&self) -> Result<()> {
        let _guard = self.lock.lock().await;
        match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    /// A usable access token, refreshing it first if it's about to expire.
    /// `force` refreshes regardless — for a token the server just rejected.
    async fn access_token(&self, client: &Client, force: bool) -> Result<String> {
        let _guard = self.lock.lock().await;
        if !self.signed_in() {
            return Err(NotConfigured("a login on /agents").into());
        }
        let mut doc = read_json(&self.path)?;
        let tokens = Tokens::read(self.provider, &doc)?;
        let fresh = tokens
            .expires_at
            .is_some_and(|t| t - Utc::now() > chrono::Duration::seconds(EXPIRY_MARGIN_SECS));
        if fresh && !force {
            return Ok(tokens.access);
        }
        let refresh = tokens
            .refresh
            .ok_or_else(|| anyhow!("access token expired and the login has no refresh token"))?;
        let granted = match self.provider {
            Provider::Claude => refresh_claude(client, &refresh).await?,
            Provider::Codex => refresh_codex(client, &refresh).await?,
        };
        granted.write(self.provider, &mut doc);
        write_json(&self.path, &doc)?;
        info!("{} login refreshed", self.provider.key());
        Ok(granted.access)
    }

    /// GET `url` with the login's bearer token, retrying once on a refused
    /// token: the expiry we track can be wrong (revoked, clock skew).
    async fn get(&self, client: &Client, url: &str, extra: &[(&str, String)]) -> Result<Value> {
        let mut force = false;
        loop {
            let token = self.access_token(client, force).await?;
            let mut req = client.get(url).bearer_auth(&token);
            for (k, v) in extra {
                req = req.header(*k, v);
            }
            if self.provider == Provider::Codex {
                if let Some(account) = codex_account_id(&read_json(&self.path)?, &token) {
                    req = req.header("ChatGPT-Account-Id", account);
                }
            }
            let resp = req.send().await?;
            let status = resp.status();
            if status == StatusCode::UNAUTHORIZED && !force {
                force = true;
                continue;
            }
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                bail!("{url}: HTTP {status}: {}", body.chars().take(200).collect::<String>());
            }
            return Ok(resp.json().await?);
        }
    }
}

fn read_json(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Temp file + rename, so a crash mid-write never leaves a torn login.
fn write_json(path: &Path, doc: &Value) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(doc)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

struct Tokens {
    access: String,
    refresh: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    /// Codex only: the OpenID token, rotated alongside the others.
    id_token: Option<String>,
}

impl Tokens {
    fn read(provider: Provider, doc: &Value) -> Result<Self> {
        let s = |v: &Value| v.as_str().filter(|s| !s.is_empty()).map(str::to_string);
        match provider {
            Provider::Claude => {
                let o = &doc["claudeAiOauth"];
                Ok(Self {
                    access: s(&o["accessToken"]).ok_or_else(|| anyhow!("no claudeAiOauth.accessToken"))?,
                    refresh: s(&o["refreshToken"]),
                    expires_at: o["expiresAt"].as_i64().and_then(DateTime::from_timestamp_millis),
                    id_token: None,
                })
            }
            Provider::Codex => {
                let t = &doc["tokens"];
                let access = s(&t["access_token"]).ok_or_else(|| anyhow!("no tokens.access_token"))?;
                Ok(Self {
                    expires_at: jwt_expiry(&access),
                    access,
                    refresh: s(&t["refresh_token"]),
                    id_token: s(&t["id_token"]),
                })
            }
        }
    }

    /// Store the tokens in the CLI's format, leaving every other field as it was.
    fn write(&self, provider: Provider, doc: &mut Value) {
        match provider {
            Provider::Claude => {
                let o = &mut doc["claudeAiOauth"];
                o["accessToken"] = json!(self.access);
                if let Some(r) = &self.refresh {
                    o["refreshToken"] = json!(r);
                }
                if let Some(t) = self.expires_at {
                    o["expiresAt"] = json!(t.timestamp_millis());
                }
            }
            Provider::Codex => {
                let t = &mut doc["tokens"];
                t["access_token"] = json!(self.access);
                if let Some(r) = &self.refresh {
                    t["refresh_token"] = json!(r);
                }
                if let Some(i) = &self.id_token {
                    t["id_token"] = json!(i);
                    if let Some(account) = jwt_claims(i)
                        .and_then(|c| c["https://api.openai.com/auth"]["chatgpt_account_id"].as_str().map(str::to_string))
                    {
                        t["account_id"] = json!(account);
                    }
                }
                doc["last_refresh"] = json!(Utc::now().to_rfc3339());
            }
        }
    }
}

/// Payload of a JWT, unverified — we only read our own token's expiry and
/// account out of it.
fn jwt_claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn jwt_expiry(token: &str) -> Option<DateTime<Utc>> {
    jwt_claims(token)?["exp"].as_i64().and_then(|e| DateTime::from_timestamp(e, 0))
}

/// The ChatGPT workspace the usage belongs to: `tokens.account_id` in
/// `auth.json`, else the claim inside the access token.
fn codex_account_id(doc: &Value, access: &str) -> Option<String> {
    doc["tokens"]["account_id"]
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            jwt_claims(access)?["https://api.openai.com/auth"]["chatgpt_account_id"]
                .as_str()
                .map(str::to_string)
        })
}

fn env_or(var: &str, default: &str) -> String {
    std::env::var(var)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

// ── OAuth: Claude ────────────────────────────────────────────────────────────
//
// Endpoints and client id are the Claude Code CLI's own (its "claude.ai"
// login with the manual redirect).

const CLAUDE_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const CLAUDE_AUTHORIZE_URL: &str = "https://claude.com/cai/oauth/authorize";
const CLAUDE_REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";
/// Reading usage needs the profile; nothing here sends prompts or makes keys.
const CLAUDE_SCOPES: &str = "user:profile user:inference";

fn claude_token_url() -> String {
    env_or("CLAUDE_OAUTH_TOKEN_URL", "https://platform.claude.com/v1/oauth/token")
}

async fn refresh_claude(client: &Client, refresh: &str) -> Result<Tokens> {
    let url = claude_token_url();
    let resp = client
        .post(&url)
        .json(&json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh,
            "client_id": CLAUDE_CLIENT_ID,
            "scope": CLAUDE_SCOPES,
        }))
        .send()
        .await?;
    claude_tokens(&token_response(resp, &url).await?)
}

fn claude_tokens(v: &Value) -> Result<Tokens> {
    Ok(Tokens {
        access: v["access_token"].as_str().ok_or_else(|| anyhow!("no access_token"))?.into(),
        refresh: v["refresh_token"].as_str().map(str::to_string),
        expires_at: v["expires_in"]
            .as_i64()
            .map(|s| Utc::now() + chrono::Duration::seconds(s)),
        id_token: None,
    })
}

// ── OAuth: Codex ─────────────────────────────────────────────────────────────
//
// The Codex CLI's client id and its `codex login --device-auth` flow.

const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CODEX_ISSUER: &str = "https://auth.openai.com";

async fn refresh_codex(client: &Client, refresh: &str) -> Result<Tokens> {
    let url = format!("{CODEX_ISSUER}/oauth/token");
    let resp = client
        .post(&url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("client_id", CODEX_CLIENT_ID),
        ])
        .send()
        .await?;
    codex_tokens(&token_response(resp, &url).await?)
}

fn codex_tokens(v: &Value) -> Result<Tokens> {
    let access: String = v["access_token"].as_str().ok_or_else(|| anyhow!("no access_token"))?.into();
    Ok(Tokens {
        expires_at: jwt_expiry(&access),
        access,
        refresh: v["refresh_token"].as_str().map(str::to_string),
        id_token: v["id_token"].as_str().map(str::to_string),
    })
}

async fn token_response(resp: reqwest::Response, url: &str) -> Result<Value> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!(
            "{url}: HTTP {status}: {} — sign in again on /agents",
            body.chars().take(200).collect::<String>()
        );
    }
    Ok(resp.json().await?)
}

// ── PKCE ─────────────────────────────────────────────────────────────────────

fn random_token(bytes: usize) -> String {
    use ring::rand::SecureRandom;
    let mut buf = vec![0u8; bytes];
    ring::rand::SystemRandom::new()
        .fill(&mut buf)
        .expect("system RNG unavailable");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

fn pkce_challenge(verifier: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest.as_ref())
}

// ── Sign-in flows ────────────────────────────────────────────────────────────

/// An unfinished sign-in, as the web UI shows it.
#[derive(Clone, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Pending {
    /// Open `authorize_url`, approve, and paste the code the page shows.
    PasteCode { authorize_url: String },
    /// Enter `user_code` at `verification_url`; the server notices by itself.
    DeviceCode { user_code: String, verification_url: String, expires_at: DateTime<Utc> },
}

struct ClaudeFlow {
    verifier: String,
    state: String,
    authorize_url: String,
}

struct CodexFlow {
    user_code: String,
    expires_at: DateTime<Utc>,
}

#[derive(Default)]
struct Flows {
    claude: Option<ClaudeFlow>,
    codex: Option<CodexFlow>,
    /// Why the last sign-in or usage pull failed, per provider.
    errors: BTreeMap<&'static str, String>,
}

/// One provider's state for the web UI.
#[derive(Serialize, ToSchema)]
pub struct LoginStatus {
    pub provider: String,
    pub signed_in: bool,
    /// Email of the signed-in account, when the provider tells us.
    pub account: Option<String>,
    pub pending: Option<Pending>,
    /// The last sign-in or usage error, cleared on the next success.
    pub error: Option<String>,
}

// ── Usage ────────────────────────────────────────────────────────────────────

const SESSION_SECS: i64 = 5 * 3600;
const WEEK_SECS: i64 = 7 * 86_400;

pub struct Agents {
    client: Client,
    claude: Login,
    codex: Login,
    flows: Mutex<Flows>,
}

impl Agents {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("http client"),
            claude: Login::new(Provider::Claude, data_dir),
            codex: Login::new(Provider::Codex, data_dir),
            flows: Mutex::new(Flows::default()),
        }
    }

    fn login(&self, provider: Provider) -> &Login {
        match provider {
            Provider::Claude => &self.claude,
            Provider::Codex => &self.codex,
        }
    }

    async fn note_error(&self, provider: Provider, result: &Result<impl Sized>) {
        let mut flows = self.flows.lock().await;
        match result {
            Ok(_) => flows.errors.remove(provider.key()),
            Err(e) if e.downcast_ref::<NotConfigured>().is_some() => flows.errors.remove(provider.key()),
            Err(e) => flows.errors.insert(provider.key(), format!("{e:#}")),
        };
    }

    pub async fn status(&self) -> Vec<LoginStatus> {
        let flows = self.flows.lock().await;
        [Provider::Claude, Provider::Codex]
            .into_iter()
            .map(|p| {
                let login = self.login(p);
                let pending = match p {
                    Provider::Claude => flows.claude.as_ref().map(|f| Pending::PasteCode {
                        authorize_url: f.authorize_url.clone(),
                    }),
                    Provider::Codex => flows.codex.as_ref().map(|f| Pending::DeviceCode {
                        user_code: f.user_code.clone(),
                        verification_url: format!("{CODEX_ISSUER}/codex/device"),
                        expires_at: f.expires_at,
                    }),
                };
                LoginStatus {
                    provider: p.key().into(),
                    signed_in: login.signed_in(),
                    account: login.account(),
                    pending,
                    error: flows.errors.get(p.key()).cloned(),
                }
            })
            .collect()
    }

    /// Begin a sign-in. Claude returns at once with a URL to open; Codex gets
    /// a device code and keeps polling in the background until it's
    /// approved, expires, or is cancelled.
    pub async fn start_login(self: &Arc<Self>, provider: Provider) -> Result<()> {
        match provider {
            Provider::Claude => {
                let verifier = random_token(32);
                let state = random_token(32);
                let mut url = reqwest::Url::parse(CLAUDE_AUTHORIZE_URL)?;
                url.query_pairs_mut()
                    .append_pair("code", "true")
                    .append_pair("client_id", CLAUDE_CLIENT_ID)
                    .append_pair("response_type", "code")
                    .append_pair("redirect_uri", CLAUDE_REDIRECT_URI)
                    .append_pair("scope", CLAUDE_SCOPES)
                    .append_pair("code_challenge", &pkce_challenge(&verifier))
                    .append_pair("code_challenge_method", "S256")
                    .append_pair("state", &state);
                let mut flows = self.flows.lock().await;
                flows.errors.remove(provider.key());
                flows.claude = Some(ClaudeFlow { verifier, state, authorize_url: url.to_string() });
                Ok(())
            }
            Provider::Codex => {
                let url = format!("{CODEX_ISSUER}/api/accounts/deviceauth/usercode");
                let resp = self
                    .client
                    .post(&url)
                    .json(&json!({ "client_id": CODEX_CLIENT_ID }))
                    .send()
                    .await?;
                let v = token_response(resp, &url).await?;
                let device_auth_id = v["device_auth_id"]
                    .as_str()
                    .ok_or_else(|| anyhow!("no device_auth_id"))?
                    .to_string();
                let user_code = v["user_code"]
                    .as_str()
                    .or_else(|| v["usercode"].as_str())
                    .ok_or_else(|| anyhow!("no user_code"))?
                    .to_string();
                let interval = v["interval"]
                    .as_u64()
                    .or_else(|| v["interval"].as_str().and_then(|s| s.trim().parse().ok()))
                    .unwrap_or(5)
                    .clamp(1, 60);
                let expires_at = v["expires_at"]
                    .as_str()
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|t| t.with_timezone(&Utc))
                    .unwrap_or_else(|| Utc::now() + chrono::Duration::minutes(15));
                {
                    let mut flows = self.flows.lock().await;
                    flows.errors.remove(provider.key());
                    flows.codex = Some(CodexFlow { user_code: user_code.clone(), expires_at });
                }
                let this = Arc::clone(self);
                tokio::spawn(async move {
                    let result = this.poll_codex(&device_auth_id, &user_code, interval, expires_at).await;
                    let mut flows = this.flows.lock().await;
                    // A newer attempt (or a cancel) owns the slot now.
                    if flows.codex.as_ref().is_none_or(|f| f.user_code != user_code) {
                        return;
                    }
                    flows.codex = None;
                    match result {
                        Ok(()) => {
                            flows.errors.remove(Provider::Codex.key());
                            info!("codex signed in");
                        }
                        Err(e) => {
                            warn!("codex sign-in failed: {e:#}");
                            flows.errors.insert(Provider::Codex.key(), format!("{e:#}"));
                        }
                    }
                });
                Ok(())
            }
        }
    }

    async fn poll_codex(&self, device_auth_id: &str, user_code: &str, interval: u64, expires_at: DateTime<Utc>) -> Result<()> {
        let url = format!("{CODEX_ISSUER}/api/accounts/deviceauth/token");
        let grant = loop {
            tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            if self.flows.lock().await.codex.as_ref().is_none_or(|f| f.user_code != user_code) {
                return Ok(()); // cancelled or superseded
            }
            if Utc::now() > expires_at {
                bail!("the code expired before it was entered — start again");
            }
            let resp = self
                .client
                .post(&url)
                .json(&json!({ "device_auth_id": device_auth_id, "user_code": user_code }))
                .send()
                .await?;
            match resp.status() {
                s if s.is_success() => break resp.json::<Value>().await?,
                // Not approved yet.
                StatusCode::FORBIDDEN | StatusCode::NOT_FOUND => continue,
                s => bail!("{url}: HTTP {s}: {}", resp.text().await.unwrap_or_default().chars().take(200).collect::<String>()),
            }
        };
        let code = grant["authorization_code"].as_str().ok_or_else(|| anyhow!("no authorization_code"))?;
        let verifier = grant["code_verifier"].as_str().ok_or_else(|| anyhow!("no code_verifier"))?;
        let token_url = format!("{CODEX_ISSUER}/oauth/token");
        let redirect = format!("{CODEX_ISSUER}/deviceauth/callback");
        let resp = self
            .client
            .post(&token_url)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", &redirect),
                ("client_id", CODEX_CLIENT_ID),
                ("code_verifier", verifier),
            ])
            .send()
            .await?;
        let tokens = codex_tokens(&token_response(resp, &token_url).await?)?;
        let mut doc = json!({ "OPENAI_API_KEY": null, "tokens": {} });
        tokens.write(Provider::Codex, &mut doc);
        self.codex.store(&doc).await
    }

    /// Finish a Claude sign-in with what the callback page showed: `code#state`
    /// (or the whole callback URL).
    pub async fn finish_claude(&self, pasted: &str) -> Result<()> {
        let flow = self
            .flows
            .lock()
            .await
            .claude
            .take()
            .ok_or_else(|| anyhow!("no sign-in in progress — start again"))?;
        let result = self.exchange_claude(&flow, pasted).await;
        let mut flows = self.flows.lock().await;
        match &result {
            Ok(()) => {
                flows.errors.remove(Provider::Claude.key());
                info!("claude signed in");
            }
            // Keep the flow so a mistyped paste can be retried.
            Err(_) => flows.claude = Some(flow),
        }
        result
    }

    async fn exchange_claude(&self, flow: &ClaudeFlow, pasted: &str) -> Result<()> {
        let (code, state) = parse_pasted_code(pasted)?;
        if state.as_deref().is_some_and(|s| s != flow.state) {
            bail!("that code belongs to a different sign-in — use the latest link");
        }
        let url = claude_token_url();
        let resp = self
            .client
            .post(&url)
            .json(&json!({
                "grant_type": "authorization_code",
                "code": code,
                "redirect_uri": CLAUDE_REDIRECT_URI,
                "client_id": CLAUDE_CLIENT_ID,
                "code_verifier": flow.verifier,
                "state": flow.state,
            }))
            .send()
            .await?;
        let v = token_response(resp, &url).await?;
        let tokens = claude_tokens(&v)?;
        let mut doc = json!({ "claudeAiOauth": { "scopes": CLAUDE_SCOPES.split(' ').collect::<Vec<_>>() } });
        tokens.write(Provider::Claude, &mut doc);
        if let Some(email) = v["account"]["email_address"].as_str() {
            doc["claudeAiOauth"]["account"] = json!(email);
        }
        self.claude.store(&doc).await
    }

    /// Drop an unfinished sign-in.
    pub async fn cancel_login(&self, provider: Provider) {
        let mut flows = self.flows.lock().await;
        match provider {
            Provider::Claude => flows.claude = None,
            Provider::Codex => flows.codex = None,
        }
    }

    pub async fn sign_out(&self, provider: Provider) -> Result<()> {
        self.cancel_login(provider).await;
        self.flows.lock().await.errors.remove(provider.key());
        self.login(provider).forget().await
    }

    pub async fn claude(&self) -> Result<AgentUsage> {
        let result = self.claude_usage().await;
        self.note_error(Provider::Claude, &result).await;
        result
    }

    async fn claude_usage(&self) -> Result<AgentUsage> {
        let url = env_or("CLAUDE_USAGE_URL", "https://api.anthropic.com/api/oauth/usage");
        let v = self
            .claude
            .get(&self.client, &url, &[("anthropic-beta", "oauth-2025-04-20".into())])
            .await?;
        parse_claude(&v)
    }

    pub async fn codex(&self) -> Result<AgentUsage> {
        let result = self.codex_usage().await;
        self.note_error(Provider::Codex, &result).await;
        result
    }

    async fn codex_usage(&self) -> Result<AgentUsage> {
        let url = env_or("CODEX_USAGE_URL", "https://chatgpt.com/backend-api/wham/usage");
        // chatgpt.com only answers clients that look like the Codex CLI.
        let v = self
            .codex
            .get(
                &self.client,
                &url,
                &[
                    ("User-Agent", "codex_cli_rs".into()),
                    ("originator", "codex_cli_rs".into()),
                ],
            )
            .await?;
        parse_codex(&v, Utc::now())
    }
}

/// `(code, state)` out of what the user pasted: the callback page's
/// `code#state`, a bare code, or the callback URL itself.
fn parse_pasted_code(pasted: &str) -> Result<(String, Option<String>)> {
    let pasted = pasted.trim();
    if pasted.is_empty() {
        bail!("paste the code the page showed after approving");
    }
    if let Ok(url) = reqwest::Url::parse(pasted) {
        let get = |k: &str| url.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.into_owned());
        let code = get("code").ok_or_else(|| anyhow!("that URL has no code in it"))?;
        return Ok((code, get("state")));
    }
    Ok(match pasted.split_once('#') {
        Some((code, state)) => (code.to_string(), Some(state.to_string())),
        None => (pasted.to_string(), None),
    })
}

/// Both providers report utilization as a percent, possibly fractional.
fn pct(v: &Value) -> u8 {
    v.as_f64().unwrap_or(0.0).round().clamp(0.0, 100.0) as u8
}

fn parse_claude(v: &Value) -> Result<AgentUsage> {
    let window = |k: &str| -> (u8, Option<DateTime<Utc>>) {
        let w = &v[k];
        let resets = w["resets_at"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&Utc));
        (pct(&w["utilization"]), resets)
    };
    if v["five_hour"].is_null() && v["seven_day"].is_null() {
        bail!("usage response has neither five_hour nor seven_day");
    }
    let (session_pct, session_resets) = window("five_hour");
    let (week_pct, week_resets) = window("seven_day");
    Ok(AgentUsage {
        name: "CLAUDE".into(),
        session_pct,
        session_resets,
        session_window_secs: SESSION_SECS,
        week_pct,
        week_resets,
        week_window_secs: WEEK_SECS,
        limited: ["five_hour", "seven_day"]
            .iter()
            .any(|k| !v[*k]["locked_reason"].is_null()),
    })
}

fn parse_codex(v: &Value, now: DateTime<Utc>) -> Result<AgentUsage> {
    let rl = &v["rate_limit"];
    if rl.is_null() {
        bail!("usage response has no rate_limit");
    }
    let window = |w: &Value, default_secs: i64| -> (u8, Option<DateTime<Utc>>, i64) {
        let resets = w["reset_at"]
            .as_i64()
            .and_then(|t| DateTime::from_timestamp(t, 0))
            .or_else(|| w["reset_after_seconds"].as_i64().map(|s| now + chrono::Duration::seconds(s)));
        let secs = w["limit_window_seconds"].as_i64().filter(|&s| s > 0).unwrap_or(default_secs);
        (pct(&w["used_percent"]), resets, secs)
    };
    let (session_pct, session_resets, session_window_secs) = window(&rl["primary_window"], SESSION_SECS);
    let (week_pct, week_resets, week_window_secs) = window(&rl["secondary_window"], WEEK_SECS);
    Ok(AgentUsage {
        name: "CODEX".into(),
        session_pct,
        session_resets,
        session_window_secs,
        week_pct,
        week_resets,
        week_window_secs,
        limited: rl["limit_reached"].as_bool() == Some(true) || rl["allowed"].as_bool() == Some(false),
    })
}

// ── Token history ────────────────────────────────────────────────────────────

/// Days kept; the panel shows seven, the rest is slack for late pushes.
const KEEP_DAYS: i64 = 31;

/// provider → source machine → day → tokens.
type History = BTreeMap<String, BTreeMap<String, BTreeMap<NaiveDate, u64>>>;

#[derive(Default, Serialize, Deserialize)]
struct HistoryFile {
    #[serde(default)]
    providers: History,
}

/// Daily token counts pushed in from the machines running the agents,
/// persisted as `$DATA_DIR/agent-tokens.json`.
pub struct TokenStore {
    path: PathBuf,
    history: Mutex<History>,
}

impl TokenStore {
    pub async fn open(data_dir: impl Into<PathBuf>) -> Self {
        let path = data_dir.into().join("agent-tokens.json");
        let history = match tokio::fs::read_to_string(&path).await {
            Ok(text) => serde_json::from_str::<HistoryFile>(&text)
                .map(|f| f.providers)
                .unwrap_or_else(|e| {
                    warn!("ignoring malformed {}: {e}", path.display());
                    History::new()
                }),
            Err(_) => History::new(),
        };
        Self {
            path,
            history: Mutex::new(history),
        }
    }

    /// Record a push. Days in the push replace what `source` said about them
    /// before (ccusage reports whole days, so the newest count wins); days it
    /// leaves out are kept. Returns how many days were recorded.
    pub async fn record(&self, provider: Provider, source: &str, body: &str) -> Result<usize> {
        let days = parse_daily(body)?;
        let mut history = self.history.lock().await;
        let per_source = history
            .entry(provider.key().to_string())
            .or_default()
            .entry(source.to_string())
            .or_default();
        per_source.extend(days.iter().copied());
        let cutoff = Local::now().date_naive() - chrono::Duration::days(KEEP_DAYS);
        for sources in history.values_mut() {
            for days in sources.values_mut() {
                days.retain(|d, _| *d >= cutoff);
            }
        }
        let file = HistoryFile { providers: history.clone() };
        if let Some(dir) = self.path.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        let tmp = self.path.with_extension("json.tmp");
        tokio::fs::write(&tmp, serde_json::to_vec_pretty(&file)?).await?;
        tokio::fs::rename(&tmp, &self.path).await?;
        Ok(days.len())
    }

    /// Tokens per day for the last seven local days, oldest first, summed
    /// over every source. `None` when nobody has pushed for `provider`.
    pub async fn week(&self, provider: Provider) -> Option<[u64; 7]> {
        let history = self.history.lock().await;
        let sources = history.get(provider.key()).filter(|s| !s.is_empty())?;
        let today = Local::now().date_naive();
        let mut out = [0u64; 7];
        for (i, slot) in out.iter_mut().enumerate() {
            let day = today - chrono::Duration::days(6 - i as i64);
            *slot = sources.values().filter_map(|d| d.get(&day)).sum();
        }
        Some(out)
    }
}

/// Day → total tokens out of `ccusage claude daily --json` /
/// `ccusage codex daily --json`: a `daily` array (or a bare array) of objects
/// with a date and a token total. Field names and date formats are read
/// leniently, since ccusage versions don't agree on them.
fn parse_daily(body: &str) -> Result<Vec<(NaiveDate, u64)>> {
    let v: Value = serde_json::from_str(body).context("body is not JSON")?;
    let rows = v["daily"]
        .as_array()
        .or_else(|| v.as_array())
        .ok_or_else(|| anyhow!("expected a `daily` array"))?;
    rows.iter()
        .map(|r| {
            let date = ["date", "period", "day"]
                .iter()
                .find_map(|k| r[*k].as_str())
                .ok_or_else(|| anyhow!("row without a date: {r}"))?;
            let date = parse_day(date).ok_or_else(|| anyhow!("unrecognized date {date:?}"))?;
            let tokens = ["totalTokens", "total_tokens", "tokens"]
                .iter()
                .find_map(|k| r[*k].as_u64())
                .ok_or_else(|| anyhow!("row without totalTokens: {r}"))?;
            Ok((date, tokens))
        })
        .collect()
}

fn parse_day(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    ["%Y-%m-%d", "%Y%m%d", "%b %d, %Y", "%B %d, %Y", "%d %b %Y"]
        .iter()
        .find_map(|f| NaiveDate::parse_from_str(s, f).ok())
        .or_else(|| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|t| Local.from_utc_datetime(&t.naive_utc()).date_naive())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_usage() {
        let v = json!({
            "five_hour": { "utilization": 42.0, "resets_at": "2026-09-30T14:40:00+00:00" },
            "seven_day": { "utilization": 44, "resets_at": "2026-10-05T07:00:00Z" },
            "seven_day_opus": null
        });
        let u = parse_claude(&v).unwrap();
        assert_eq!((u.session_pct, u.week_pct), (42, 44));
        assert!(u.session_resets.is_some() && u.week_resets.is_some());
        assert!(!u.is_limited());
    }

    #[test]
    fn idle_claude_window_has_no_reset() {
        let v = json!({ "five_hour": { "utilization": 0, "resets_at": null },
                        "seven_day": { "utilization": 3, "resets_at": "2026-10-05T07:00:00Z" } });
        let u = parse_claude(&v).unwrap();
        assert_eq!(u.session_resets, None);
        assert_eq!(u.session_projection(Utc::now()), None);
    }

    #[test]
    fn parses_codex_usage() {
        let now = DateTime::from_timestamp(1_790_000_000, 0).unwrap();
        let v = json!({ "plan_type": "plus", "rate_limit": {
            "allowed": true, "limit_reached": false,
            "primary_window": { "used_percent": 23, "limit_window_seconds": 18000, "reset_after_seconds": 3600 },
            "secondary_window": { "used_percent": 61, "limit_window_seconds": 604800, "reset_at": 1_790_100_000 }
        }});
        let u = parse_codex(&v, now).unwrap();
        assert_eq!((u.session_pct, u.week_pct), (23, 61));
        assert_eq!(u.session_resets, Some(now + chrono::Duration::hours(1)));
        assert_eq!(u.week_window_secs, 604_800);
        assert!(!u.limited);
        let mut limited = v.clone();
        limited["rate_limit"]["limit_reached"] = json!(true);
        assert!(parse_codex(&limited, now).unwrap().is_limited());
    }

    #[test]
    fn projection_and_pace() {
        let now = Utc::now();
        let u = AgentUsage {
            name: "CLAUDE".into(),
            session_pct: 40,
            // Halfway through the 5h window.
            session_resets: Some(now + chrono::Duration::minutes(150)),
            session_window_secs: SESSION_SECS,
            week_pct: 44,
            // A quarter of the week gone.
            week_resets: Some(now + chrono::Duration::hours(126)),
            week_window_secs: WEEK_SECS,
            limited: false,
        };
        assert_eq!(u.session_projection(now), Some(80));
        assert_eq!(u.week_pace(now), Some(25));
    }

    #[test]
    fn pasted_codes_in_every_shape() {
        assert_eq!(parse_pasted_code(" abc#xyz \n").unwrap(), ("abc".into(), Some("xyz".into())));
        assert_eq!(parse_pasted_code("abc").unwrap(), ("abc".into(), None));
        assert_eq!(
            parse_pasted_code("https://platform.claude.com/oauth/code/callback?code=abc&state=xyz").unwrap(),
            ("abc".into(), Some("xyz".into()))
        );
        assert!(parse_pasted_code("  ").is_err());
    }

    #[test]
    fn pkce_matches_rfc7636() {
        // Appendix B of RFC 7636.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_ne!(random_token(32), random_token(32));
    }

    #[tokio::test]
    async fn claude_sign_in_builds_a_pkce_link() {
        let dir = std::env::temp_dir().join(format!("trmnl-login-{}", std::process::id()));
        let agents = Arc::new(Agents::new(&dir));
        agents.start_login(Provider::Claude).await.unwrap();
        let status = agents.status().await;
        let Some(Pending::PasteCode { authorize_url }) = &status[0].pending else { panic!("no pending claude flow") };
        let url = reqwest::Url::parse(authorize_url).unwrap();
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["redirect_uri"], CLAUDE_REDIRECT_URI);
        assert_eq!(q["code_challenge_method"], "S256");
        assert!(!status[0].signed_in);
        // A code from another attempt is refused before anything is sent.
        let err = agents.finish_claude("abc#not-our-state").await.unwrap_err();
        assert!(err.to_string().contains("different sign-in"), "{err}");
        assert!(agents.status().await[0].pending.is_some(), "a bad paste can be retried");
        agents.cancel_login(Provider::Claude).await;
        assert!(agents.status().await[0].pending.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reads_both_ccusage_shapes() {
        let claude = r#"{"daily":[{"date":"2026-09-29","inputTokens":1,"totalTokens":6300000},
                                  {"date":"2026-09-30","totalTokens":4100000}],"totals":{}}"#;
        assert_eq!(
            parse_daily(claude).unwrap(),
            vec![
                (NaiveDate::from_ymd_opt(2026, 9, 29).unwrap(), 6_300_000),
                (NaiveDate::from_ymd_opt(2026, 9, 30).unwrap(), 4_100_000),
            ]
        );
        let codex = r#"{"daily":[{"period":"Sep 30, 2026","totalTokens":700000}]}"#;
        assert_eq!(
            parse_daily(codex).unwrap(),
            vec![(NaiveDate::from_ymd_opt(2026, 9, 30).unwrap(), 700_000)]
        );
        assert!(parse_daily(r#"{"daily":[{"date":"yesterday","totalTokens":1}]}"#).is_err());
    }

    #[tokio::test]
    async fn sources_sum_and_repushes_replace() {
        let dir = std::env::temp_dir().join(format!("trmnl-tokens-{}", std::process::id()));
        let store = TokenStore::open(&dir).await;
        let today = Local::now().date_naive().format("%Y-%m-%d");
        let push = |n: u64| format!(r#"{{"daily":[{{"date":"{today}","totalTokens":{n}}}]}}"#);
        store.record(Provider::Claude, "laptop", &push(100)).await.unwrap();
        store.record(Provider::Claude, "desktop", &push(50)).await.unwrap();
        store.record(Provider::Claude, "laptop", &push(300)).await.unwrap();
        assert_eq!(store.week(Provider::Claude).await.unwrap()[6], 350);
        assert_eq!(store.week(Provider::Codex).await, None);
        let reopened = TokenStore::open(&dir).await;
        assert_eq!(reopened.week(Provider::Claude).await.unwrap()[6], 350);
        let _ = std::fs::remove_dir_all(dir);
    }

}
