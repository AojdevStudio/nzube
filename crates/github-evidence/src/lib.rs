//! Read-only GitHub evidence retrieval for Nzube.
//!
//! `client` fetches repository evidence through a closed set of GET endpoints on
//! api.github.com. `auth` is a separate API for the GitHub App device flow; the
//! evidence client never sends POST and never talks to github.com.

pub mod auth;
pub mod classify;
pub mod client;
pub mod request;
pub mod secret;
pub mod transport;
