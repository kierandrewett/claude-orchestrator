/// OAuth 2.0 PKCE client for MCP servers that require browser authentication.
///
/// Flow:
///  1. `start_flow(url, redirect_uri)` → discovers auth server, registers client,
///     returns an authorization URL the user must visit.
///  2. User visits the URL, authorises, and is redirected to `redirect_uri?code=...&state=...`.
///  3. `exchange_code(pending, code)` → exchanges the code for an access token.
use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ── PKCE helpers ───────────────────────────────────────────────────────────────

fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    getrandom::getrandom(&mut buf).expect("getrandom failed");
    buf
}

fn pkce_pair() -> (String, String) {
    let verifier = URL_SAFE_NO_PAD.encode(random_bytes(32));
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn random_state() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes(16))
}

// ── Discovery ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ResourceMetadata {
    authorization_servers: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct AuthServerMetadata {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: Option<String>,
}

async fn fetch_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T> {
    reqwest::get(url)
        .await
        .with_context(|| format!("GET {url}"))?
        .json::<T>()
        .await
        .with_context(|| format!("parsing JSON from {url}"))
}

/// Discover the OAuth authorization server for an MCP resource URL.
async fn discover(resource_url: &str) -> Result<AuthServerMetadata> {
    // Step 1: resource metadata → find authorization server
    let resource_meta_url = {
        let base = resource_url.trim_end_matches('/');
        // Try the standard .well-known path
        format!(
            "{}/.well-known/oauth-protected-resource",
            base_origin(base)?
        )
    };

    let resource_meta: ResourceMetadata = fetch_json(&resource_meta_url).await?;
    let auth_server = resource_meta
        .authorization_servers
        .and_then(|v| v.into_iter().next())
        .with_context(|| format!("no authorization_servers in {resource_meta_url}"))?;

    // Step 2: authorization server metadata
    let auth_meta: AuthServerMetadata = fetch_json(&format!(
        "{}/.well-known/oauth-authorization-server",
        auth_server.trim_end_matches('/')
    ))
    .await?;

    Ok(auth_meta)
}

fn base_origin(url: &str) -> Result<String> {
    // Parse just the scheme + host + port
    let parsed = url::Url::parse(url).with_context(|| format!("invalid URL: {url}"))?;
    Ok(format!(
        "{}://{}",
        parsed.scheme(),
        parsed.host_str().context("no host in URL")?
    ))
}

// ── Client registration ────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ClientRegRequest<'a> {
    client_name: &'a str,
    redirect_uris: &'a [&'a str],
    grant_types: &'a [&'a str],
    response_types: &'a [&'a str],
    token_endpoint_auth_method: &'a str,
    code_challenge_methods_supported: &'a [&'a str],
}

#[derive(Debug, Deserialize)]
struct ClientRegResponse {
    client_id: String,
    #[allow(dead_code)]
    client_secret: Option<String>,
}

async fn register_client(
    reg_endpoint: &str,
    redirect_uri: &str,
) -> Result<String> {
    let resp = reqwest::Client::new()
        .post(reg_endpoint)
        .json(&ClientRegRequest {
            client_name: "Claude Orchestrator",
            redirect_uris: &[redirect_uri],
            grant_types: &["authorization_code", "refresh_token"],
            response_types: &["code"],
            token_endpoint_auth_method: "none",
            code_challenge_methods_supported: &["S256"],
        })
        .send()
        .await
        .context("client registration request")?;

    if !resp.status().is_success() {
        bail!(
            "client registration failed: {} {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
    }

    let reg: ClientRegResponse = resp.json().await.context("parsing registration response")?;
    Ok(reg.client_id)
}

// ── Public API ─────────────────────────────────────────────────────────────────

/// State that must be persisted between the redirect and the callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingOAuth {
    pub server_name: String,
    pub code_verifier: String,
    pub redirect_uri: String,
    pub token_endpoint: String,
    pub client_id: String,
}

/// Token response from a successful exchange or refresh.
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
}

/// Initiate the OAuth flow for an MCP server URL.
///
/// Returns `(auth_url, pending_state)`. The caller must:
///   - Store `pending_state` keyed by the `state` query param in `auth_url`.
///   - Send the user to `auth_url`.
pub async fn start_flow(
    server_name: &str,
    resource_url: &str,
    redirect_uri: &str,
) -> Result<(String, PendingOAuth)> {
    let meta = discover(resource_url)
        .await
        .with_context(|| format!("OAuth discovery for {resource_url}"))?;

    let client_id = match meta.registration_endpoint.as_deref() {
        Some(reg_ep) => register_client(reg_ep, redirect_uri).await?,
        None => bail!("OAuth server has no registration_endpoint; manual client_id required"),
    };

    let (verifier, challenge) = pkce_pair();
    let state = random_state();

    let mut auth_url = url::Url::parse(&meta.authorization_endpoint)
        .context("parsing authorization_endpoint")?;
    auth_url.query_pairs_mut()
        .append_pair("client_id", &client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state);

    let pending = PendingOAuth {
        server_name: server_name.to_string(),
        code_verifier: verifier,
        redirect_uri: redirect_uri.to_string(),
        token_endpoint: meta.token_endpoint,
        client_id,
    };

    Ok((format!("{auth_url}"), pending))
}

/// Exchange an authorization code for tokens.
pub async fn exchange_code(
    pending: &PendingOAuth,
    code: &str,
) -> Result<TokenResponse> {
    let mut params = HashMap::new();
    params.insert("grant_type", "authorization_code");
    params.insert("code", code);
    params.insert("redirect_uri", &pending.redirect_uri);
    params.insert("code_verifier", &pending.code_verifier);
    params.insert("client_id", &pending.client_id);

    let resp = reqwest::Client::new()
        .post(&pending.token_endpoint)
        .form(&params)
        .send()
        .await
        .context("token exchange request")?;

    if !resp.status().is_success() {
        bail!(
            "token exchange failed: {} {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
    }

    resp.json::<TokenResponse>().await.context("parsing token response")
}

/// Use a refresh token to get a new access token.
pub async fn refresh_token(
    token_endpoint: &str,
    client_id: &str,
    refresh: &str,
) -> Result<TokenResponse> {
    let mut params = HashMap::new();
    params.insert("grant_type", "refresh_token");
    params.insert("refresh_token", refresh);
    params.insert("client_id", client_id);

    let resp = reqwest::Client::new()
        .post(token_endpoint)
        .form(&params)
        .send()
        .await
        .context("token refresh request")?;

    if !resp.status().is_success() {
        bail!(
            "token refresh failed: {} {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
    }

    resp.json::<TokenResponse>().await.context("parsing refresh response")
}
