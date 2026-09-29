//! Secure-storage and disconnect boundary checks. Payloads are synthetic.

use std::collections::VecDeque;
use std::sync::Mutex;

use github_evidence::auth::{AuthError, ClientId, DeviceFlow, UserGrant};
use github_evidence::client::Credential;
use github_evidence::connection::{
    Connection, ConnectionError, REVOKE_URL, ReauthReason, SecretStore, StoreError, StoredGrant,
};
use github_evidence::secret::SecretToken;
use github_evidence::transport::{FormValue, HttpRequest, HttpResponse, Transport, TransportError};

const NOW: u64 = 1_900_000_000;
const ACCESS: &str = "ghu_STORED_ACCESS_FIXTURE";
const REFRESH: &str = "ghr_STORED_REFRESH_FIXTURE";

#[derive(Default)]
struct Fixture {
    replies: Mutex<VecDeque<Result<HttpResponse, TransportError>>>,
    seen: Mutex<Vec<HttpRequest>>,
}

impl Fixture {
    fn with(replies: impl IntoIterator<Item = (u16, &'static str)>) -> Self {
        let replies = replies
            .into_iter()
            .map(|(status, body)| {
                Ok(HttpResponse {
                    status,
                    headers: Vec::new(),
                    body: body.as_bytes().to_vec(),
                    body_truncated: false,
                })
            })
            .collect();
        Self {
            replies: Mutex::new(replies),
            seen: Mutex::default(),
        }
    }
}

impl Transport for &Fixture {
    async fn send(&self, request: &HttpRequest, _: usize) -> Result<HttpResponse, TransportError> {
        self.seen.lock().unwrap().push(request.clone());
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("fixture ran out of replies")
    }
}

/// Stands in for platform secure storage: it holds serialized bytes, as a keychain
/// item would, and can be told to fail.
#[derive(Default)]
struct MemoryKeychain {
    item: Mutex<Option<String>>,
    fail_load: bool,
    fail_save: bool,
}

impl SecretStore for &MemoryKeychain {
    fn load(&self) -> Result<Option<StoredGrant>, StoreError> {
        if self.fail_load {
            return Err(StoreError::Unavailable);
        }
        let item = self.item.lock().unwrap();
        let Some(raw) = item.as_deref() else {
            return Ok(None);
        };
        let fields: Vec<&str> = raw.split('\n').collect();
        let [access, access_exp, refresh, refresh_exp] = fields.as_slice() else {
            return Err(StoreError::Corrupt);
        };
        let num = |s: &str| {
            (!s.is_empty())
                .then(|| s.parse().map_err(|_| StoreError::Corrupt))
                .transpose()
        };
        Ok(Some(StoredGrant {
            access_token: SecretToken::new(*access),
            access_expires_at: num(access_exp)?,
            refresh_token: (!refresh.is_empty()).then(|| SecretToken::new(*refresh)),
            refresh_expires_at: num(refresh_exp)?,
        }))
    }

    fn save(&self, grant: &StoredGrant) -> Result<(), StoreError> {
        if self.fail_save {
            return Err(StoreError::WriteFailed);
        }
        let opt = |n: Option<u64>| n.map(|n| n.to_string()).unwrap_or_default();
        *self.item.lock().unwrap() = Some(format!(
            "{}\n{}\n{}\n{}",
            grant.access_token.expose_secret(),
            opt(grant.access_expires_at),
            grant
                .refresh_token
                .as_ref()
                .map(SecretToken::expose_secret)
                .unwrap_or_default(),
            opt(grant.refresh_expires_at),
        ));
        Ok(())
    }

    fn delete(&self) -> Result<(), StoreError> {
        *self.item.lock().unwrap() = None;
        Ok(())
    }
}

impl MemoryKeychain {
    fn raw(&self) -> Option<String> {
        self.item.lock().unwrap().clone()
    }
}

fn grant(access_secs: Option<u64>, refresh: Option<(&str, u64)>) -> UserGrant {
    UserGrant {
        access_token: SecretToken::new(ACCESS),
        access_expires_in: access_secs.map(std::time::Duration::from_secs),
        refresh_token: refresh.map(|(t, _)| SecretToken::new(t)),
        refresh_expires_in: refresh.map(|(_, s)| std::time::Duration::from_secs(s)),
    }
}

fn connection<'a>(
    store: &'a MemoryKeychain,
    fixture: &'a Fixture,
) -> Connection<&'a MemoryKeychain, &'a Fixture> {
    Connection::new(
        store,
        DeviceFlow::new(fixture, ClientId::parse("Iv23liFIXTURE000").unwrap()),
    )
}

fn token_of(credential: Credential) -> SecretToken {
    match credential {
        Credential::UserToken(token) => token,
        Credential::Anonymous => panic!("expected a user token"),
    }
}

const ROTATED: &str = r#"{"access_token":"ghu_ROTATED_ACCESS","expires_in":28800,"refresh_token":"ghr_ROTATED_REFRESH","refresh_token_expires_in":15897600,"scope":"","token_type":"bearer"}"#;

#[tokio::test]
async fn stored_grant_is_used_until_expiry_without_network() {
    let store = MemoryKeychain::default();
    let fixture = Fixture::default();
    let conn = connection(&store, &fixture);
    conn.connect(&grant(Some(28_800), Some((REFRESH, 15_897_600))), NOW)
        .unwrap();
    assert!(token_of(conn.credential(NOW + 1_000).await.unwrap()).matches(ACCESS));
    assert!(fixture.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn expired_access_refreshes_without_secret_and_rotates_storage() {
    let store = MemoryKeychain::default();
    let fixture = Fixture::with([(200, ROTATED)]);
    let conn = connection(&store, &fixture);
    conn.connect(&grant(Some(28_800), Some((REFRESH, 15_897_600))), NOW)
        .unwrap();

    let later = NOW + 28_800;
    assert!(token_of(conn.credential(later).await.unwrap()).matches("ghu_ROTATED_ACCESS"));
    let raw = store.raw().unwrap();
    assert!(raw.contains("ghr_ROTATED_REFRESH") && !raw.contains(REFRESH) && !raw.contains(ACCESS));
    let seen = fixture.seen.lock().unwrap();
    let keys: Vec<&str> = seen[0].form.iter().map(|(k, _)| *k).collect();
    assert_eq!(keys, ["client_id", "grant_type", "refresh_token"]);
    assert!(matches!(&seen[0].form[2].1, FormValue::Secret(s) if s.matches(REFRESH)));
}

#[tokio::test]
async fn expiry_skew_refreshes_just_before_the_recorded_expiry() {
    let store = MemoryKeychain::default();
    let fixture = Fixture::with([(200, ROTATED)]);
    let conn = connection(&store, &fixture);
    conn.connect(&grant(Some(28_800), Some((REFRESH, 15_897_600))), NOW)
        .unwrap();
    conn.credential(NOW + 28_800 - 30).await.unwrap();
    assert_eq!(
        fixture.seen.lock().unwrap().len(),
        1,
        "refreshed inside the 60s skew"
    );
}

/// Initial grant, refresh replies, expected reason, expected refresh requests.
type ReauthCase = (UserGrant, Vec<(u16, &'static str)>, ReauthReason, usize);

#[tokio::test]
async fn unusable_refresh_clears_storage_and_requires_sign_in() {
    let cases: [ReauthCase; 4] = [
        (
            grant(Some(10), None),
            vec![],
            ReauthReason::NoRefreshToken,
            0,
        ),
        (
            grant(Some(10), Some((REFRESH, 100))),
            vec![],
            ReauthReason::RefreshExpired,
            0,
        ),
        (
            grant(Some(10), Some((REFRESH, 15_897_600))),
            vec![(200, r#"{"error":"bad_refresh_token"}"#)],
            ReauthReason::RefreshRejected,
            1,
        ),
        (
            grant(Some(10), Some((REFRESH, 15_897_600))),
            vec![(401, "{}")],
            ReauthReason::RefreshRejected,
            1,
        ),
    ];
    for (initial, replies, reason, requests) in cases {
        let store = MemoryKeychain::default();
        let fixture = Fixture::with(replies);
        let conn = connection(&store, &fixture);
        conn.connect(&initial, NOW).unwrap();
        assert_eq!(
            conn.credential(NOW + 1_000).await.unwrap_err(),
            ConnectionError::ReauthRequired(reason)
        );
        assert_eq!(store.raw(), None, "{reason:?} left tokens in storage");
        assert_eq!(fixture.seen.lock().unwrap().len(), requests, "{reason:?}");
    }
}

#[tokio::test]
async fn transient_refresh_failure_keeps_the_stored_grant() {
    let store = MemoryKeychain::default();
    let fixture = Fixture::with([(503, "{}")]);
    let conn = connection(&store, &fixture);
    conn.connect(&grant(Some(10), Some((REFRESH, 15_897_600))), NOW)
        .unwrap();
    assert_eq!(
        conn.credential(NOW + 1_000).await.unwrap_err(),
        ConnectionError::Refresh(AuthError::Status(503))
    );
    assert!(store.raw().unwrap().contains(REFRESH));
}

#[tokio::test]
async fn storage_failures_surface_and_never_fall_back() {
    let fixture = Fixture::default();
    let unavailable = MemoryKeychain {
        fail_load: true,
        ..MemoryKeychain::default()
    };
    assert_eq!(
        connection(&unavailable, &fixture)
            .credential(NOW)
            .await
            .unwrap_err(),
        ConnectionError::Store(StoreError::Unavailable)
    );

    let read_only = MemoryKeychain {
        fail_save: true,
        ..MemoryKeychain::default()
    };
    let conn = connection(&read_only, &fixture);
    assert_eq!(
        conn.connect(&grant(Some(28_800), None), NOW).unwrap_err(),
        ConnectionError::Store(StoreError::WriteFailed)
    );
    assert_eq!(
        conn.credential(NOW).await.unwrap_err(),
        ConnectionError::NotConnected
    );
    assert!(fixture.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn rotated_token_is_not_returned_when_it_cannot_be_saved() {
    let store = MemoryKeychain::default();
    let fixture = Fixture::with([(200, ROTATED)]);
    connection(&store, &fixture)
        .connect(&grant(Some(10), Some((REFRESH, 15_897_600))), NOW)
        .unwrap();
    let failing = MemoryKeychain {
        item: Mutex::new(store.raw()),
        fail_save: true,
        ..MemoryKeychain::default()
    };
    let err = connection(&failing, &fixture)
        .credential(NOW + 1_000)
        .await
        .unwrap_err();
    assert_eq!(err, ConnectionError::Store(StoreError::WriteFailed));
    assert!(!format!("{err:?} {err}").contains("ghu_ROTATED_ACCESS"));
}

#[tokio::test]
async fn disconnect_is_local_and_says_so() {
    let store = MemoryKeychain::default();
    let fixture = Fixture::default();
    let conn = connection(&store, &fixture);
    conn.connect(&grant(Some(28_800), Some((REFRESH, 15_897_600))), NOW)
        .unwrap();
    let outcome = conn.disconnect().unwrap();
    assert!(!outcome.remote_grant_revoked);
    assert_eq!(outcome.revoke_url, REVOKE_URL);
    assert_eq!(store.raw(), None);
    assert_eq!(
        conn.credential(NOW).await.unwrap_err(),
        ConnectionError::NotConnected
    );
    assert!(
        fixture.seen.lock().unwrap().is_empty(),
        "disconnect made no network call"
    );
}

#[test]
fn stored_grant_debug_is_redacted() {
    let stored = StoredGrant::from_grant(&grant(Some(1), Some((REFRESH, 2))), NOW);
    let rendered = format!("{stored:?}");
    assert!(
        !rendered.contains(ACCESS) && !rendered.contains(REFRESH),
        "{rendered}"
    );
    assert_eq!(stored.access_expires_at, Some(NOW + 1));
}
