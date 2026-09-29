use std::fmt;

/// A credential value (access token, refresh token, device code).
///
/// Debug is redacted and there is no Display, so formatting an error, request,
/// or evidence record never prints it. The raw value is readable through
/// `expose_secret`, which exists for platform secure-storage adapters; callers of
/// that method must keep the value out of logs, telemetry, exports, and prompts.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretToken(String);

impl SecretToken {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The raw value, for `connection::SecretStore` adapters that write platform secure
    /// storage. Never log, display, or export it.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    /// Lets tests and callers check which credential was attached without reading it out.
    pub fn matches(&self, candidate: &str) -> bool {
        self.0 == candidate
    }
}

impl fmt::Debug for SecretToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretToken([redacted])")
    }
}
