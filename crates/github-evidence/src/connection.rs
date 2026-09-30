//! Product-auth lifecycle over platform secure storage.
//!
//! The application supplies a `SecretStore` backed by the platform's protected
//! storage (Keychain, Keystore-backed storage, Secret Service, Credential Manager).
//! There is no file, environment, or ambient-login fallback. Time is passed in as
//! Unix seconds so the lifecycle is deterministic and testable.

use crate::auth::{AuthError, DeviceFlow, UserGrant};
use crate::client::Credential;
use crate::secret::SecretToken;
use crate::transport::Transport;

/// Where a user revokes the grant on GitHub. A secretless client cannot do this itself.
pub const REVOKE_URL: &str = "https://github.com/settings/apps/authorizations";
/// Refresh this many seconds before the recorded expiry, to absorb clock skew.
pub const EXPIRY_SKEW_SECS: u64 = 60;

/// A user grant as persisted. Expiry times are absolute Unix seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredGrant {
    pub access_token: SecretToken,
    pub access_expires_at: Option<u64>,
    pub refresh_token: Option<SecretToken>,
    pub refresh_expires_at: Option<u64>,
}

impl StoredGrant {
    pub fn from_grant(grant: &UserGrant, now: u64) -> Self {
        let at = |d: Option<std::time::Duration>| d.map(|d| now.saturating_add(d.as_secs()));
        Self {
            access_token: grant.access_token.clone(),
            access_expires_at: at(grant.access_expires_in),
            refresh_token: grant.refresh_token.clone(),
            refresh_expires_at: at(grant.refresh_expires_in),
        }
    }

    fn access_valid(&self, now: u64) -> bool {
        self.access_expires_at
            .is_none_or(|exp| now.saturating_add(EXPIRY_SKEW_SECS) < exp)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("secure storage is unavailable")]
    Unavailable,
    #[error("secure storage rejected the write")]
    WriteFailed,
    #[error("stored credential is unreadable")]
    Corrupt,
}

/// Implemented by the application over platform secure storage. One grant per store.
pub trait SecretStore {
    fn load(&self) -> Result<Option<StoredGrant>, StoreError>;
    fn save(&self, grant: &StoredGrant) -> Result<(), StoreError>;
    fn delete(&self) -> Result<(), StoreError>;
    /// Replaces the stored grant with `next`, or deletes it when `next` is `None`, only if
    /// the store still holds exactly `expected` (every token and expiry time). Returns
    /// whether it did. The compare and the write must be one atomic step: this is what
    /// stops a stale caller from overwriting or deleting a newer grant, and an in-flight
    /// refresh from bringing back a grant after a disconnect.
    fn replace_if_current(
        &self,
        expected: &StoredGrant,
        next: Option<&StoredGrant>,
    ) -> Result<bool, StoreError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReauthReason {
    NoRefreshToken,
    RefreshExpired,
    /// GitHub refused the refresh token (revoked, already used, or invalid).
    RefreshRejected,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectionError {
    #[error("GitHub is not connected")]
    NotConnected,
    /// Stored tokens were cleared; the user must run the device flow again.
    #[error("GitHub sign-in required again: {0:?}")]
    ReauthRequired(ReauthReason),
    #[error("secure storage: {0}")]
    Store(StoreError),
    /// The stored grant changed while this call ran and the newer grant is not usable
    /// at `now` either. Call `credential` again.
    #[error("the stored GitHub grant changed during this call; try again")]
    Superseded,
    /// A refresh failure that is not a rejection, such as 429, 5xx, or a transport error.
    /// The stored grant is kept for a later retry.
    #[error("token refresh failed: {0}")]
    Refresh(AuthError),
}

/// Result of a local disconnect. The grant on GitHub is untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disconnected {
    pub remote_grant_revoked: bool,
    pub revoke_url: &'static str,
}

pub struct Connection<S, T> {
    store: S,
    flow: DeviceFlow<T>,
}

impl<S: SecretStore, T: Transport> Connection<S, T> {
    pub fn new(store: S, flow: DeviceFlow<T>) -> Self {
        Self { store, flow }
    }

    /// Persists a grant from the device flow. It is usable only once the save succeeds.
    pub fn connect(&self, grant: &UserGrant, now: u64) -> Result<(), ConnectionError> {
        self.store
            .save(&StoredGrant::from_grant(grant, now))
            .map_err(ConnectionError::Store)
    }

    /// A credential valid at `now`. An expired access token is refreshed without a
    /// client secret, and the rotated grant is saved before it is returned.
    pub async fn credential(&self, now: u64) -> Result<Credential, ConnectionError> {
        let stored = self
            .store
            .load()
            .map_err(ConnectionError::Store)?
            .ok_or(ConnectionError::NotConnected)?;
        if stored.access_valid(now) {
            return Ok(Credential::UserToken(stored.access_token));
        }
        let Some(refresh_token) = &stored.refresh_token else {
            return self.retire(&stored, ReauthReason::NoRefreshToken, now);
        };
        if stored.refresh_expires_at.is_some_and(|exp| now >= exp) {
            return self.retire(&stored, ReauthReason::RefreshExpired, now);
        }
        let rotated = match self.flow.refresh(refresh_token).await {
            Ok(grant) => StoredGrant::from_grant(&grant, now),
            Err(error) if is_rejection(&error) => {
                return self.retire(&stored, ReauthReason::RefreshRejected, now);
            }
            Err(retryable) => return Err(ConnectionError::Refresh(retryable)),
        };
        // Save only if the grant this call loaded is still the stored one. A disconnect or
        // newer sign-in during the refresh wins, and the rotated token is then discarded.
        // GitHub has already invalidated the old refresh token, so a failed save means the
        // rotated token is never handed out unsaved.
        if self
            .store
            .replace_if_current(&stored, Some(&rotated))
            .map_err(ConnectionError::Store)?
        {
            return Ok(Credential::UserToken(rotated.access_token));
        }
        self.current_after_race(now)
    }

    /// Local disconnect: deletes stored tokens. Revocation on GitHub is the user's
    /// action at `REVOKE_URL`.
    pub fn disconnect(&self) -> Result<Disconnected, StoreError> {
        self.store.delete()?;
        Ok(Disconnected {
            remote_grant_revoked: false,
            revoke_url: REVOKE_URL,
        })
    }

    /// Deletes `stored` because it can no longer produce a token, unless another caller
    /// replaced or removed it after this call loaded it.
    fn retire(
        &self,
        stored: &StoredGrant,
        reason: ReauthReason,
        now: u64,
    ) -> Result<Credential, ConnectionError> {
        if self
            .store
            .replace_if_current(stored, None)
            .map_err(ConnectionError::Store)?
        {
            return Err(ConnectionError::ReauthRequired(reason));
        }
        self.current_after_race(now)
    }

    /// The store changed while this call ran. An empty store means a disconnect, which wins.
    fn current_after_race(&self, now: u64) -> Result<Credential, ConnectionError> {
        match self.store.load().map_err(ConnectionError::Store)? {
            None => Err(ConnectionError::NotConnected),
            Some(current) if current.access_valid(now) => {
                Ok(Credential::UserToken(current.access_token))
            }
            Some(_) => Err(ConnectionError::Superseded),
        }
    }
}

/// Only an explicit refusal deletes the stored grant: an OAuth error body, or 400 or 401
/// from the token endpoint. Throttling (429), other statuses, transport failures, and
/// malformed responses keep it for a retry, because deleting a still-valid refresh token
/// would disconnect the user for a failure GitHub did not attribute to the grant.
fn is_rejection(error: &AuthError) -> bool {
    match error {
        AuthError::IncorrectClientCredentials
        | AuthError::DeviceFlowDisabled
        | AuthError::AccessDenied
        | AuthError::Expired
        | AuthError::NotAppUserToken
        | AuthError::UnknownOAuthError
        | AuthError::Status(400 | 401) => true,
        AuthError::InvalidClientId
        | AuthError::Status(_)
        | AuthError::Transport
        | AuthError::Malformed => false,
    }
}
