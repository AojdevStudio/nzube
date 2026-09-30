//! GitHub App device flow, separate from evidence retrieval.
//!
//! Needs only the app's public client ID. Token requests and device-flow refresh
//! carry no client secret (docs: "Refreshing user access tokens", client_secret
//! "Required unless the user access token was generated using the device flow").

use std::time::Duration;

use serde::Deserialize;
use url::Url;

use crate::secret::SecretToken;
use crate::transport::{FormValue, HttpRequest, Method, Transport};

pub const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
pub const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// Documented prefix of GitHub App user access tokens. OAuth App tokens use `gho_`.
const APP_USER_TOKEN_PREFIX: &str = "ghu_";

/// The registered GitHub App's client ID. Public; never a secret. There is no default value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientId(String);

impl ClientId {
    pub fn parse(input: &str) -> Result<Self, AuthError> {
        let input = input.trim();
        if (8..=64).contains(&input.len())
            && input
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.')
        {
            Ok(Self(input.to_owned()))
        } else {
            Err(AuthError::InvalidClientId)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("client ID is not in a valid format")]
    InvalidClientId,
    #[error("GitHub rejected the client ID")]
    IncorrectClientCredentials,
    #[error("device flow is not enabled in the GitHub App settings")]
    DeviceFlowDisabled,
    #[error("the user cancelled authorization")]
    AccessDenied,
    #[error("the device code expired; start again")]
    Expired,
    #[error("GitHub issued a token that is not a GitHub App user token")]
    NotAppUserToken,
    #[error("unrecognized OAuth error code")]
    UnknownOAuthError,
    #[error("unexpected status {0}")]
    Status(u16),
    #[error("transport failure")]
    Transport,
    #[error("malformed response")]
    Malformed,
}

#[derive(Debug, Clone)]
pub struct DeviceAuthorization {
    pub device_code: SecretToken,
    /// Shown to the user; not a credential by itself.
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: Duration,
    pub interval: Duration,
}

#[derive(Debug, Clone)]
pub struct UserGrant {
    pub access_token: SecretToken,
    pub access_expires_in: Option<Duration>,
    pub refresh_token: Option<SecretToken>,
    pub refresh_expires_in: Option<Duration>,
}

#[derive(Debug, Clone)]
pub enum PollOutcome {
    Pending,
    SlowDown { interval: Duration },
    Granted(UserGrant),
}

pub struct DeviceFlow<T> {
    transport: T,
    client_id: ClientId,
}

impl<T: Transport> DeviceFlow<T> {
    pub fn new(transport: T, client_id: ClientId) -> Self {
        Self {
            transport,
            client_id,
        }
    }

    /// POST /login/device/code
    pub async fn start(&self) -> Result<DeviceAuthorization, AuthError> {
        #[derive(Deserialize)]
        struct Wire {
            device_code: String,
            user_code: String,
            verification_uri: String,
            expires_in: u64,
            interval: u64,
        }
        let body = self
            .post(DEVICE_CODE_URL, vec![("client_id", self.client_plain())])
            .await?;
        if let Some(error) = oauth_error(&body) {
            return Err(error);
        }
        let wire: Wire = serde_json::from_slice(&body).map_err(|_| AuthError::Malformed)?;
        Ok(DeviceAuthorization {
            device_code: SecretToken::new(wire.device_code),
            user_code: wire.user_code,
            verification_uri: wire.verification_uri,
            expires_in: Duration::from_secs(wire.expires_in),
            interval: Duration::from_secs(wire.interval),
        })
    }

    /// One poll of POST /login/oauth/access_token. Callers wait `interval` between polls.
    pub async fn poll(&self, device_code: &SecretToken) -> Result<PollOutcome, AuthError> {
        let body = self
            .post(
                TOKEN_URL,
                vec![
                    ("client_id", self.client_plain()),
                    ("device_code", FormValue::Secret(device_code.clone())),
                    ("grant_type", FormValue::Plain(DEVICE_GRANT.to_owned())),
                ],
            )
            .await?;
        #[derive(Deserialize)]
        struct Pending {
            error: String,
            interval: Option<u64>,
        }
        if let Ok(pending) = serde_json::from_slice::<Pending>(&body) {
            match pending.error.as_str() {
                "authorization_pending" => return Ok(PollOutcome::Pending),
                "slow_down" => {
                    let interval =
                        Duration::from_secs(pending.interval.ok_or(AuthError::Malformed)?);
                    return Ok(PollOutcome::SlowDown { interval });
                }
                _ => {}
            }
        }
        if let Some(error) = oauth_error(&body) {
            return Err(error);
        }
        grant(&body).map(PollOutcome::Granted)
    }

    /// Polls until granted, denied, or expired, honouring `interval` and `slow_down`.
    pub async fn wait_for_grant(
        &self,
        authorization: &DeviceAuthorization,
    ) -> Result<UserGrant, AuthError> {
        let deadline = tokio::time::Instant::now() + authorization.expires_in;
        let mut interval = authorization.interval;
        loop {
            tokio::time::sleep(interval).await;
            if tokio::time::Instant::now() >= deadline {
                return Err(AuthError::Expired);
            }
            match self.poll(&authorization.device_code).await? {
                PollOutcome::Pending => {}
                PollOutcome::SlowDown { interval: next } => interval = next,
                PollOutcome::Granted(grant) => return Ok(grant),
            }
        }
    }

    /// Device-flow refresh: client_id + refresh_token, no client secret.
    pub async fn refresh(&self, refresh_token: &SecretToken) -> Result<UserGrant, AuthError> {
        let body = self
            .post(
                TOKEN_URL,
                vec![
                    ("client_id", self.client_plain()),
                    ("grant_type", FormValue::Plain("refresh_token".to_owned())),
                    ("refresh_token", FormValue::Secret(refresh_token.clone())),
                ],
            )
            .await?;
        if let Some(error) = oauth_error(&body) {
            return Err(error);
        }
        grant(&body)
    }

    fn client_plain(&self) -> FormValue {
        FormValue::Plain(self.client_id.0.clone())
    }

    async fn post(
        &self,
        url: &str,
        form: Vec<(&'static str, FormValue)>,
    ) -> Result<Vec<u8>, AuthError> {
        let request = HttpRequest {
            method: Method::Post,
            url: Url::parse(url).expect("static auth URL parses"),
            headers: vec![("accept", "application/json".to_owned())],
            bearer: None,
            form,
        };
        let response = self
            .transport
            .send(&request, 64 << 10)
            .await
            .map_err(|_| AuthError::Transport)?;
        // GitHub reports OAuth errors with 200; anything else non-2xx is unexpected.
        if !(200..300).contains(&response.status) {
            return Err(AuthError::Status(response.status));
        }
        Ok(response.body)
    }
}

fn oauth_error(body: &[u8]) -> Option<AuthError> {
    #[derive(Deserialize)]
    struct Wire {
        error: String,
    }
    let wire: Wire = serde_json::from_slice(body).ok()?;
    Some(match wire.error.as_str() {
        "incorrect_client_credentials" => AuthError::IncorrectClientCredentials,
        "device_flow_disabled" => AuthError::DeviceFlowDisabled,
        "access_denied" => AuthError::AccessDenied,
        // The docs' table says `expired_token`; its prose says `token_expired`.
        "expired_token" | "token_expired" => AuthError::Expired,
        _ => AuthError::UnknownOAuthError,
    })
}

fn grant(body: &[u8]) -> Result<UserGrant, AuthError> {
    #[derive(Deserialize)]
    struct Wire {
        access_token: String,
        expires_in: Option<u64>,
        refresh_token: Option<String>,
        refresh_token_expires_in: Option<u64>,
    }
    let wire: Wire = serde_json::from_slice(body).map_err(|_| AuthError::Malformed)?;
    if !wire.access_token.starts_with(APP_USER_TOKEN_PREFIX) {
        return Err(AuthError::NotAppUserToken);
    }
    Ok(UserGrant {
        access_token: SecretToken::new(wire.access_token),
        access_expires_in: wire.expires_in.map(Duration::from_secs),
        refresh_token: wire.refresh_token.map(SecretToken::new),
        refresh_expires_in: wire.refresh_token_expires_in.map(Duration::from_secs),
    })
}
