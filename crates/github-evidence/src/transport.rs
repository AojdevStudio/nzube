use std::future::Future;
use std::time::Duration;

use url::Url;

use crate::secret::SecretToken;

pub const API_HOST: &str = "api.github.com";
/// Pinned REST API version: the newest supported version listed in GitHub's "API Versions" docs on 2026-09-29.
pub const API_VERSION: &str = "2026-03-10";
pub const USER_AGENT: &str = concat!("nzube-github-evidence/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Method {
    Get,
    Post,
}

#[derive(Debug, Clone)]
pub enum FormValue {
    Plain(String),
    Secret(SecretToken),
}

/// One outbound HTTP call. Evidence requests are always GET with no form; only
/// `auth` builds POSTs, and only to github.com/login.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub url: Url,
    pub headers: Vec<(&'static str, String)>,
    pub bearer: Option<SecretToken>,
    pub form: Vec<(&'static str, FormValue)>,
}

impl HttpRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    /// Lowercased names.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The body exceeded the cap the caller passed and was cut there.
    pub body_truncated: bool,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, serde::Serialize)]
pub enum TransportError {
    #[error("request timed out")]
    Timeout,
    #[error("could not connect")]
    Connect,
    #[error("transport failed")]
    Other,
}

/// The seam between request logic and the network. Fixture transports implement it in tests.
pub trait Transport: Sync {
    fn send(
        &self,
        request: &HttpRequest,
        max_body_bytes: usize,
    ) -> impl Future<Output = Result<HttpResponse, TransportError>> + Send;
}

/// Real network transport. Redirects are never followed, so a credential cannot
/// be replayed to a host the request did not name.
pub struct ReqwestTransport {
    http: reqwest::Client,
}

impl ReqwestTransport {
    pub fn new() -> Result<Self, TransportError> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .user_agent(USER_AGENT)
            .build()
            .map_err(|_| TransportError::Other)?;
        Ok(Self { http })
    }
}

impl Transport for ReqwestTransport {
    async fn send(
        &self,
        request: &HttpRequest,
        max_body_bytes: usize,
    ) -> Result<HttpResponse, TransportError> {
        let method = match request.method {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
        };
        let mut builder = self.http.request(method, request.url.clone());
        for (name, value) in &request.headers {
            builder = builder.header(*name, value);
        }
        if let Some(token) = &request.bearer {
            builder = builder.bearer_auth(token.expose());
        }
        if !request.form.is_empty() {
            let pairs: Vec<(&str, &str)> = request
                .form
                .iter()
                .map(|(k, v)| match v {
                    FormValue::Plain(s) => (*k, s.as_str()),
                    FormValue::Secret(s) => (*k, s.expose()),
                })
                .collect();
            builder = builder.form(&pairs);
        }

        let mut response = builder.send().await.map_err(map_reqwest)?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                Some((k.as_str().to_ascii_lowercase(), v.to_str().ok()?.to_owned()))
            })
            .collect();

        let mut body = Vec::new();
        let mut body_truncated = false;
        while let Some(chunk) = response.chunk().await.map_err(map_reqwest)? {
            let room = max_body_bytes - body.len();
            if chunk.len() > room {
                body.extend_from_slice(&chunk[..room]);
                body_truncated = true;
                break;
            }
            body.extend_from_slice(&chunk);
        }
        Ok(HttpResponse {
            status,
            headers,
            body,
            body_truncated,
        })
    }
}

// Deliberately drops reqwest's message: it can carry URLs and upstream detail.
fn map_reqwest(error: reqwest::Error) -> TransportError {
    if error.is_timeout() {
        TransportError::Timeout
    } else if error.is_connect() {
        TransportError::Connect
    } else {
        TransportError::Other
    }
}
