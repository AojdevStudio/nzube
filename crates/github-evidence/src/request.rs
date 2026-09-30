//! User input parsing and the closed endpoint allowlist.
//!
//! Every value that reaches a URL is parsed here first. `Endpoint` is the only
//! way the evidence client builds a URL, so the allowlist is the enum itself.

use std::fmt;
use std::num::NonZeroU64;

use url::Url;

use crate::transport::API_HOST;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    #[error("repository must be owner/repo or an https://github.com/owner/repo URL")]
    RepoShape,
    #[error("repository URL must point at github.com")]
    ForeignHost,
    #[error("invalid owner name")]
    Owner,
    #[error("invalid repository name")]
    RepoName,
    #[error(
        "path must be relative, non-empty, and free of '.', '..', empty segments, and control characters"
    )]
    Path,
    #[error("commit must be a full 40-character hex SHA")]
    CommitSha,
    #[error("invalid git ref")]
    GitRef,
    #[error("number must be a positive integer")]
    Number,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RepoRef {
    owner: String,
    name: String,
}

impl RepoRef {
    /// Accepts `owner/repo`, `https://github.com/owner/repo`, optional `.git`, and
    /// trailing browser path segments such as `/pull/3`. Anything naming another
    /// host, a port, or credentials is rejected.
    pub fn parse(input: &str) -> Result<Self, InputError> {
        let input = input.trim();
        let rest = if input.contains("://") {
            let url = Url::parse(input).map_err(|_| InputError::RepoShape)?;
            if url.scheme() != "https"
                || url.host_str() != Some("github.com")
                || url.port().is_some()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(InputError::ForeignHost);
            }
            url.path().trim_start_matches('/').to_owned()
        } else if input.starts_with("github.com/") {
            return Self::parse(&format!("https://{input}"));
        } else {
            input.to_owned()
        };

        let mut parts = rest.split('/');
        let (Some(owner), Some(name)) = (parts.next(), parts.next()) else {
            return Err(InputError::RepoShape);
        };
        // Bare `owner/repo` must be exactly two segments; URLs may carry more.
        if !input.contains("://") && parts.next().is_some() {
            return Err(InputError::RepoShape);
        }
        let name = name.strip_suffix(".git").unwrap_or(name);
        let owner_ok = (1..=39).contains(&owner.len())
            && !owner.starts_with('-')
            && owner
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if !owner_ok {
            return Err(InputError::Owner);
        }
        let name_ok = (1..=100).contains(&name.len())
            && name != "."
            && name != ".."
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
        if !name_ok {
            return Err(InputError::RepoName);
        }
        Ok(Self {
            owner: owner.to_owned(),
            name: name.to_owned(),
        })
    }
}

impl fmt::Display for RepoRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

/// A repository-relative file path. Segments are percent-encoded when placed in a URL,
/// so `?`, `#`, and `%` stay inside the path.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RepoPath(String);

impl RepoPath {
    pub fn parse(input: &str) -> Result<Self, InputError> {
        let ok = !input.is_empty()
            && input.len() <= 4096
            && !input.starts_with('/')
            && !input.contains('\\')
            && !input.chars().any(char::is_control)
            && input
                .split('/')
                .all(|s| !s.is_empty() && s != "." && s != "..");
        if ok {
            Ok(Self(input.to_owned()))
        } else {
            Err(InputError::Path)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CommitSha(String);

impl CommitSha {
    pub fn parse(input: &str) -> Result<Self, InputError> {
        if input.len() == 40 && input.bytes().all(|b| b.is_ascii_hexdigit()) {
            Ok(Self(input.to_ascii_lowercase()))
        } else {
            Err(InputError::CommitSha)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A branch or tag name to resolve into a `CommitSha`. Only sent as a query value.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GitRef(String);

impl GitRef {
    pub fn parse(input: &str) -> Result<Self, InputError> {
        let ok = (1..=255).contains(&input.len())
            && !input.starts_with(['-', '/'])
            && !input.ends_with(['/', '.'])
            && !input.contains("..")
            && !input.contains("@{")
            && !input
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c));
        if ok {
            Ok(Self(input.to_owned()))
        } else {
            Err(InputError::GitRef)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

macro_rules! number_type {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
        pub struct $name(NonZeroU64);

        impl $name {
            pub fn parse(input: &str) -> Result<Self, InputError> {
                input
                    .trim()
                    .trim_start_matches('#')
                    .parse()
                    .map(Self)
                    .map_err(|_| InputError::Number)
            }

            pub fn get(self) -> u64 {
                self.0.get()
            }
        }
    };
}
number_type!(IssueNumber);
number_type!(PullNumber);

/// The two commits a pull request's files and diff are computed from. Both can move:
/// the head on a push or force-push, the base when the base branch advances or is retargeted.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PullRevisions {
    pub base: CommitSha,
    pub head: CommitSha,
}

/// Instruction files checked, in order, for `EvidenceRequest::InstructionsFile`.
pub const INSTRUCTION_CANDIDATES: [&str; 3] =
    ["AGENTS.md", "CLAUDE.md", ".github/copilot-instructions.md"];

/// Everything Nzube may ask GitHub for. There is no variant for arbitrary methods or URLs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceRequest {
    /// Pin a branch or tag to the commit it currently names.
    ResolveRevision {
        git_ref: GitRef,
    },
    File {
        path: RepoPath,
        at: CommitSha,
    },
    /// First of `INSTRUCTION_CANDIDATES` present at the commit.
    InstructionsFile {
        at: CommitSha,
    },
    Issue {
        number: IssueNumber,
    },
    /// Conversation comments; also the top-level conversation of a pull request.
    IssueComments {
        number: IssueNumber,
    },
    PullRequest {
        number: PullNumber,
    },
    /// Files of pull request `number` as GitHub reports them now. The endpoint is
    /// mutable, so the evidence carries no revision label and is never `Complete`:
    /// full coverage is reported as `Partial(MutableSourceUnpinned)`.
    PullRequestFiles {
        number: PullNumber,
    },
    /// The pull request's diff as GitHub renders it now. Mutable, like `PullRequestFiles`.
    PullRequestDiff {
        number: PullNumber,
    },
    /// Files changed between two fixed commits, read from the compare endpoint pinned to
    /// both SHAs, so the content is determined by `revisions`. This is a comparison of
    /// that commit pair; it is not presented as GitHub's pull request view.
    CompareFiles {
        revisions: PullRevisions,
    },
    /// The diff text between the same two fixed commits.
    CompareDiff {
        revisions: PullRevisions,
    },
    History {
        from: CommitSha,
        path: Option<RepoPath>,
    },
}

/// The exact set of REST calls the evidence client can construct. All are GET on api.github.com.
#[derive(Debug, Clone, Copy)]
pub enum Endpoint<'a> {
    /// GET /repos/{owner}/{repo}/commits?sha=&path=&per_page=
    ListCommits {
        sha: &'a str,
        path: Option<&'a RepoPath>,
        per_page: u8,
    },
    /// GET /repos/{owner}/{repo}/contents/{path}?ref=
    Contents {
        path: &'a RepoPath,
        at: &'a CommitSha,
    },
    /// GET /repos/{owner}/{repo}/issues/{issue_number}
    Issue { number: IssueNumber },
    /// GET /repos/{owner}/{repo}/issues/{issue_number}/comments?per_page=
    IssueComments { number: IssueNumber, per_page: u8 },
    /// GET /repos/{owner}/{repo}/pulls/{pull_number}
    Pull { number: PullNumber },
    /// GET /repos/{owner}/{repo}/pulls/{pull_number}/files?per_page=
    PullFiles { number: PullNumber, per_page: u8 },
    /// GET /repos/{owner}/{repo}/pulls/{pull_number} with the diff media type
    PullDiff { number: PullNumber },
    /// GET /repos/{owner}/{repo}/compare/{base}...{head}?per_page=1. `per_page=1` keeps
    /// the commit list to one entry; GitHub lists changed files on the first page only.
    Compare { revisions: &'a PullRevisions },
    /// GET /repos/{owner}/{repo}/compare/{base}...{head} with the diff media type
    CompareDiff { revisions: &'a PullRevisions },
}

pub const ACCEPT_JSON: &str = "application/vnd.github+json";
pub const ACCEPT_DIFF: &str = "application/vnd.github.diff";

impl Endpoint<'_> {
    pub fn url(&self, repo: &RepoRef) -> Url {
        let mut url = Url::parse(&format!("https://{API_HOST}/")).expect("static base URL parses");
        {
            let mut segments = url
                .path_segments_mut()
                .expect("https URL has path segments");
            segments.extend(["repos", repo.owner.as_str(), repo.name.as_str()]);
            match self {
                Endpoint::ListCommits { .. } => {
                    segments.push("commits");
                }
                Endpoint::Contents { path, .. } => {
                    segments.push("contents").extend(path.0.split('/'));
                }
                Endpoint::Issue { number } => {
                    segments.extend(["issues", &number.get().to_string()]);
                }
                Endpoint::IssueComments { number, .. } => {
                    segments.extend(["issues", &number.get().to_string(), "comments"]);
                }
                Endpoint::Pull { number } | Endpoint::PullDiff { number } => {
                    segments.extend(["pulls", &number.get().to_string()]);
                }
                Endpoint::PullFiles { number, .. } => {
                    segments.extend(["pulls", &number.get().to_string(), "files"]);
                }
                Endpoint::Compare { revisions } | Endpoint::CompareDiff { revisions } => {
                    let basehead =
                        format!("{}...{}", revisions.base.as_str(), revisions.head.as_str());
                    segments.extend(["compare", basehead.as_str()]);
                }
            }
        }
        {
            let mut query = url.query_pairs_mut();
            match self {
                Endpoint::ListCommits {
                    sha,
                    path,
                    per_page,
                } => {
                    query.append_pair("sha", sha);
                    if let Some(path) = path {
                        query.append_pair("path", path.as_str());
                    }
                    query.append_pair("per_page", &per_page.to_string());
                }
                Endpoint::Contents { at, .. } => {
                    query.append_pair("ref", at.as_str());
                }
                Endpoint::IssueComments { per_page, .. } | Endpoint::PullFiles { per_page, .. } => {
                    query.append_pair("per_page", &per_page.to_string());
                }
                Endpoint::Compare { .. } => {
                    query.append_pair("per_page", "1");
                }
                Endpoint::Issue { .. }
                | Endpoint::Pull { .. }
                | Endpoint::PullDiff { .. }
                | Endpoint::CompareDiff { .. } => {}
            }
        }
        if url.query() == Some("") {
            url.set_query(None);
        }
        url
    }

    pub fn accept(&self) -> &'static str {
        match self {
            Endpoint::PullDiff { .. } | Endpoint::CompareDiff { .. } => ACCEPT_DIFF,
            _ => ACCEPT_JSON,
        }
    }
}
