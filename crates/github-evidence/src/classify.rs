//! Maps a GitHub response to a typed outcome. Pure: no I/O, no body text escapes.

use crate::transport::{HttpResponse, TransportError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum RateLimitKind {
    /// `x-ratelimit-remaining: 0`; wait until `reset_epoch`.
    Primary,
    /// Secondary limit message or `retry-after`; honour `retry_after_secs`, else wait at least a minute.
    Secondary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum DenialReason {
    /// Organization SAML SSO has not authorized this credential (`x-github-sso` header).
    SsoRequired,
    /// The token's grant lacks the permission ("Resource not accessible by ...").
    NotGrantedToToken,
    Other,
}

/// Public failure outcome. Display text is fixed or numeric: never a token, URL, or response body.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, serde::Serialize)]
pub enum FetchError {
    #[error("credential rejected (401): missing, expired, or revoked")]
    InvalidCredentials,
    #[error("access denied (403): {reason:?}")]
    PermissionDenied { reason: DenialReason },
    #[error(
        "rate limited ({kind:?}); retry_after_secs={retry_after_secs:?} reset_epoch={reset_epoch:?}"
    )]
    RateLimited {
        kind: RateLimitKind,
        retry_after_secs: Option<u64>,
        reset_epoch: Option<u64>,
    },
    /// 404 means absent OR not visible to this credential. It never proves nonexistence.
    #[error("not found or not visible to this credential (404)")]
    Unavailable,
    #[error("resource moved ({status}); redirects are not followed")]
    Moved { status: u16 },
    #[error("gone (410): disabled feature or unsupported API version")]
    Gone,
    #[error("unexpected status {status}")]
    UnexpectedStatus { status: u16 },
    #[error("transport: {0}")]
    Transport(TransportError),
    #[error("malformed response: {0}")]
    Malformed(&'static str),
    #[error("single response exceeded the {limit}-byte cap")]
    ResponseTooLarge { limit: usize },
    #[error("the {limit}-byte total budget for this fetch was already spent")]
    ByteBudgetExhausted { limit: usize },
    #[error("limit `{field}` must be greater than zero")]
    InvalidLimits { field: &'static str },
    #[error("next-page link had no usable sequential page number")]
    PaginationUnrecognized,
}

/// Rules follow docs.github.com "Rate limits for the REST API" (exceeding the rate limit):
/// primary = 403/429 with `x-ratelimit-remaining: 0`; secondary = 403/429 with a
/// secondary-limit message or `retry-after`. Any other 403 is a permission denial.
pub fn classify(response: &HttpResponse) -> Result<(), FetchError> {
    let header_u64 = |name| {
        response
            .header(name)
            .and_then(|v| v.trim().parse::<u64>().ok())
    };
    // Read internally to pick a variant; never copied into the error.
    let message = serde_json::from_slice::<serde_json::Value>(&response.body)
        .ok()
        .and_then(|v| v.get("message")?.as_str().map(str::to_ascii_lowercase))
        .unwrap_or_default();

    match response.status {
        200..=299 => Ok(()),
        301 | 302 | 303 | 307 | 308 => Err(FetchError::Moved {
            status: response.status,
        }),
        401 => Err(FetchError::InvalidCredentials),
        403 | 429 => {
            let retry_after_secs = header_u64("retry-after");
            let reset_epoch = header_u64("x-ratelimit-reset");
            if response.header("x-ratelimit-remaining").map(str::trim) == Some("0") {
                Err(FetchError::RateLimited {
                    kind: RateLimitKind::Primary,
                    retry_after_secs,
                    reset_epoch,
                })
            } else if retry_after_secs.is_some()
                || message.contains("secondary rate limit")
                || response.status == 429
            {
                Err(FetchError::RateLimited {
                    kind: RateLimitKind::Secondary,
                    retry_after_secs,
                    reset_epoch: None,
                })
            } else if response.header("x-github-sso").is_some() {
                Err(FetchError::PermissionDenied {
                    reason: DenialReason::SsoRequired,
                })
            } else if message.contains("resource not accessible by") {
                Err(FetchError::PermissionDenied {
                    reason: DenialReason::NotGrantedToToken,
                })
            } else {
                Err(FetchError::PermissionDenied {
                    reason: DenialReason::Other,
                })
            }
        }
        404 => Err(FetchError::Unavailable),
        410 => Err(FetchError::Gone),
        status => Err(FetchError::UnexpectedStatus { status }),
    }
}
