//! The read-only evidence client. It builds requests only from `Endpoint`, caps pages
//! and bytes, and reports partial retrieval instead of pretending it got everything.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use url::Url;

use crate::classify::{FetchError, classify};
use crate::request::{
    CommitSha, Endpoint, EvidenceRequest, GitRef, INSTRUCTION_CANDIDATES, PullRevisions, RepoPath,
    RepoRef,
};
use crate::secret::SecretToken;
use crate::transport::{API_VERSION, HttpRequest, HttpResponse, Method, Transport};

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Limits {
    pub per_page: u8,
    pub max_pages: u32,
    pub max_response_bytes: usize,
    pub max_total_bytes: usize,
}

impl Limits {
    /// Every limit must be positive; a zero would either send nothing useful or loop on nothing.
    fn zero_field(&self) -> Option<&'static str> {
        [
            ("per_page", self.per_page == 0),
            ("max_pages", self.max_pages == 0),
            ("max_response_bytes", self.max_response_bytes == 0),
            ("max_total_bytes", self.max_total_bytes == 0),
        ]
        .into_iter()
        .find_map(|(field, zero)| zero.then_some(field))
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            per_page: 100,
            max_pages: 10,
            max_response_bytes: 2 << 20,
            max_total_bytes: 8 << 20,
        }
    }
}

/// Which identity the client presents. There is no ambient fallback: the caller chooses.
#[derive(Debug, Clone)]
pub enum Credential {
    Anonymous,
    UserToken(SecretToken),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum PartialReason {
    PageCap {
        max_pages: u32,
    },
    ByteCap {
        limit: usize,
    },
    StoppedBy(FetchError),
    /// The content came from a mutable pull request endpoint, so it cannot be attributed
    /// to a specific base and head. Used in place of `Complete` for those endpoints.
    MutableSourceUnpinned,
    /// The compare endpoint lists at most 300 changed files. A list that reaches the
    /// limit may be cut short, and the response carries no total to check against.
    CompareFileLimit {
        listed: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum Completeness {
    Complete,
    Partial(PartialReason),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Paged<T> {
    pub items: Vec<T>,
    pub pages_fetched: u32,
    pub completeness: Completeness,
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum FileContent {
    Text(String),
    Binary {
        bytes: usize,
    },
    /// GitHub omits inline content above 1 MB; the file exists but was not retrieved.
    NotInlined,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileEvidence {
    pub path: RepoPath,
    pub at: CommitSha,
    pub blob_sha: String,
    pub size: u64,
    pub content: FileContent,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IssueDoc {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub is_pull_request: bool,
    pub body: Option<String>,
    pub comments: u64,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommentDoc {
    pub id: u64,
    pub author_association: String,
    pub body: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PullDoc {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub merged: bool,
    /// Pass these to `PullRequestFiles` and `PullRequestDiff` to bind them to this read.
    pub revisions: PullRevisions,
    pub changed_files: u64,
    pub body: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PullFileDoc {
    pub filename: String,
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
    /// Absent for binary or very large changes.
    pub patch: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommitDoc {
    pub sha: String,
    pub message: String,
    pub authored_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum Evidence {
    Revision {
        requested: GitRef,
        sha: CommitSha,
    },
    File(FileEvidence),
    /// `absent` lists candidates that returned 404 at `at`: missing or not visible, not proven missing.
    Instructions {
        at: CommitSha,
        found: Option<FileEvidence>,
        absent: Vec<RepoPath>,
    },
    Issue(IssueDoc),
    /// Pages are read one after another, so the list is not an atomic snapshot of a
    /// conversation that can change meanwhile. Each page's receipt records when it was read
    /// and the SHA-256 of its body.
    IssueComments(Paged<CommentDoc>),
    PullRequest(PullDoc),
    /// From the mutable pull request endpoint. No revision label; never `Complete`.
    PullRequestFiles(Paged<PullFileDoc>),
    /// From the mutable pull request endpoint. No revision label; never `Complete`.
    PullRequestDiff {
        text: String,
        completeness: Completeness,
    },
    /// From compare pinned to `revisions`: the revisions the content actually came from.
    CompareFiles {
        revisions: PullRevisions,
        files: Paged<PullFileDoc>,
    },
    /// From compare pinned to `revisions`. Binary changes have no text patch in a diff.
    CompareDiff {
        revisions: PullRevisions,
        text: String,
        completeness: Completeness,
    },
    History(Paged<CommitDoc>),
}

/// Redacted provenance for one HTTP call: no credential, no response body.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Receipt {
    pub method: Method,
    pub host: String,
    pub path_and_query: String,
    pub accept: &'static str,
    pub api_version: &'static str,
    pub authenticated: bool,
    pub status: Option<u16>,
    pub github_request_id: Option<String>,
    pub ratelimit_resource: Option<String>,
    pub ratelimit_remaining: Option<String>,
    pub body_bytes: usize,
    pub body_sha256: Option<String>,
    pub body_truncated: bool,
    /// When the response arrived, in milliseconds since the Unix epoch. 0 if nothing arrived.
    pub retrieved_at_unix_ms: u64,
    pub outcome: String,
}

/// The compare endpoint's documented ceiling on listed changed files.
pub const COMPARE_FILE_LIMIT: u64 = 300;

#[derive(Debug)]
pub struct Fetched {
    pub repo: RepoRef,
    pub result: Result<Evidence, FetchError>,
    pub receipts: Vec<Receipt>,
}

/// Per-`fetch` state: receipts so far and bytes read against `Limits::max_total_bytes`.
#[derive(Default)]
struct Session {
    receipts: Vec<Receipt>,
    bytes_read: usize,
}

/// A response body read under a byte cap. `limit` names the configured cap that cut it.
enum Body {
    Full(HttpResponse),
    Truncated {
        response: HttpResponse,
        limit: usize,
    },
}

pub struct EvidenceClient<T> {
    transport: T,
    repo: RepoRef,
    credential: Credential,
    limits: Limits,
}

impl<T: Transport> EvidenceClient<T> {
    pub fn new(transport: T, repo: RepoRef, credential: Credential, limits: Limits) -> Self {
        Self {
            transport,
            repo,
            credential,
            limits,
        }
    }

    pub async fn fetch(&self, request: &EvidenceRequest) -> Fetched {
        if let Some(field) = self.limits.zero_field() {
            return Fetched {
                repo: self.repo.clone(),
                result: Err(FetchError::InvalidLimits { field }),
                receipts: Vec::new(),
            };
        }
        let mut session = Session::default();
        let result = self.dispatch(request, &mut session).await;
        Fetched {
            repo: self.repo.clone(),
            result,
            receipts: session.receipts,
        }
    }

    async fn dispatch(
        &self,
        request: &EvidenceRequest,
        session: &mut Session,
    ) -> Result<Evidence, FetchError> {
        let per_page = self.limits.per_page;
        match request {
            EvidenceRequest::ResolveRevision { git_ref } => {
                let endpoint = Endpoint::ListCommits {
                    sha: git_ref.as_str(),
                    path: None,
                    per_page: 1,
                };
                let commits: Vec<wire::Commit> = self.get_one(endpoint, session).await?;
                let first = commits
                    .into_iter()
                    .next()
                    .ok_or(FetchError::Malformed("empty commit list"))?;
                let sha = CommitSha::parse(&first.sha)
                    .map_err(|_| FetchError::Malformed("commit sha"))?;
                Ok(Evidence::Revision {
                    requested: git_ref.clone(),
                    sha,
                })
            }
            EvidenceRequest::File { path, at } => {
                self.file(path, at, session).await.map(Evidence::File)
            }
            EvidenceRequest::InstructionsFile { at } => {
                let mut absent = Vec::new();
                for candidate in INSTRUCTION_CANDIDATES {
                    let path = RepoPath::parse(candidate).expect("static candidate path is valid");
                    match self.file(&path, at, session).await {
                        Ok(found) => {
                            return Ok(Evidence::Instructions {
                                at: at.clone(),
                                found: Some(found),
                                absent,
                            });
                        }
                        Err(FetchError::Unavailable) => absent.push(path),
                        Err(other) => return Err(other),
                    }
                }
                Ok(Evidence::Instructions {
                    at: at.clone(),
                    found: None,
                    absent,
                })
            }
            EvidenceRequest::Issue { number } => {
                let issue: wire::Issue = self
                    .get_one(Endpoint::Issue { number: *number }, session)
                    .await?;
                Ok(Evidence::Issue(issue.into()))
            }
            EvidenceRequest::IssueComments { number } => {
                let endpoint = Endpoint::IssueComments {
                    number: *number,
                    per_page,
                };
                let paged = self.get_paged::<wire::Comment>(endpoint, session).await?;
                Ok(Evidence::IssueComments(map_paged(paged)))
            }
            EvidenceRequest::PullRequest { number } => {
                let pull: wire::Pull = self
                    .get_one(Endpoint::Pull { number: *number }, session)
                    .await?;
                Ok(Evidence::PullRequest(pull.try_into()?))
            }
            EvidenceRequest::PullRequestFiles { number } => {
                let endpoint = Endpoint::PullFiles {
                    number: *number,
                    per_page,
                };
                let mut paged: Paged<PullFileDoc> =
                    map_paged(self.get_paged::<wire::PullFile>(endpoint, session).await?);
                paged.completeness = unpinned(paged.completeness);
                Ok(Evidence::PullRequestFiles(paged))
            }
            EvidenceRequest::PullRequestDiff { number } => {
                let (text, completeness) = self
                    .text(Endpoint::PullDiff { number: *number }, session)
                    .await?;
                Ok(Evidence::PullRequestDiff {
                    text,
                    completeness: unpinned(completeness),
                })
            }
            EvidenceRequest::CompareFiles { revisions } => {
                let compare: wire::Compare = self
                    .get_one(Endpoint::Compare { revisions }, session)
                    .await?;
                let listed = compare.files.len() as u64;
                let completeness = if listed >= COMPARE_FILE_LIMIT {
                    Completeness::Partial(PartialReason::CompareFileLimit { listed })
                } else {
                    Completeness::Complete
                };
                Ok(Evidence::CompareFiles {
                    revisions: revisions.clone(),
                    files: Paged {
                        items: compare.files.into_iter().map(PullFileDoc::from).collect(),
                        pages_fetched: 1,
                        completeness,
                    },
                })
            }
            EvidenceRequest::CompareDiff { revisions } => {
                let (text, completeness) = self
                    .text(Endpoint::CompareDiff { revisions }, session)
                    .await?;
                Ok(Evidence::CompareDiff {
                    revisions: revisions.clone(),
                    text,
                    completeness,
                })
            }
            EvidenceRequest::History { from, path } => {
                let endpoint = Endpoint::ListCommits {
                    sha: from.as_str(),
                    path: path.as_ref(),
                    per_page,
                };
                let paged = self.get_paged::<wire::Commit>(endpoint, session).await?;
                Ok(Evidence::History(map_paged(paged)))
            }
        }
    }

    async fn file(
        &self,
        path: &RepoPath,
        at: &CommitSha,
        session: &mut Session,
    ) -> Result<FileEvidence, FetchError> {
        let entry: wire::ContentEntry = self
            .get_one(Endpoint::Contents { path, at }, session)
            .await?;
        let wire::ContentEntry::File {
            sha,
            size,
            encoding,
            content,
        } = entry
        else {
            return Err(FetchError::Malformed("path is not a file"));
        };
        let content = match (encoding.as_deref(), content) {
            (Some("base64"), Some(encoded)) => {
                let compact: String = encoded.split_whitespace().collect();
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(compact)
                    .map_err(|_| FetchError::Malformed("file content base64"))?;
                match String::from_utf8(bytes) {
                    Ok(text) => FileContent::Text(text),
                    Err(e) => FileContent::Binary {
                        bytes: e.into_bytes().len(),
                    },
                }
            }
            (Some("none"), _) => FileContent::NotInlined,
            _ => return Err(FetchError::Malformed("file content encoding")),
        };
        Ok(FileEvidence {
            path: path.clone(),
            at: at.clone(),
            blob_sha: sha,
            size,
            content,
        })
    }

    /// Reads a diff body. A body cut by a byte cap is returned as partial text.
    async fn text(
        &self,
        endpoint: Endpoint<'_>,
        session: &mut Session,
    ) -> Result<(String, Completeness), FetchError> {
        let url = endpoint.url(&self.repo);
        let (response, completeness) = match self.send(&url, endpoint.accept(), session).await? {
            Body::Full(response) => (response, Completeness::Complete),
            Body::Truncated { response, limit } => (
                response,
                Completeness::Partial(PartialReason::ByteCap { limit }),
            ),
        };
        Ok((
            String::from_utf8_lossy(&response.body).into_owned(),
            completeness,
        ))
    }

    async fn get_one<W: DeserializeOwned>(
        &self,
        endpoint: Endpoint<'_>,
        session: &mut Session,
    ) -> Result<W, FetchError> {
        let response = match self
            .send(&endpoint.url(&self.repo), endpoint.accept(), session)
            .await?
        {
            Body::Full(response) => response,
            Body::Truncated { limit, .. } => return Err(FetchError::ResponseTooLarge { limit }),
        };
        serde_json::from_slice(&response.body)
            .map_err(|_| FetchError::Malformed("unexpected JSON shape"))
    }

    /// Follows `Link: rel="next"` by page number only (see `next_link`).
    /// An error before the first page is returned as an error; after it, as a partial result.
    async fn get_paged<W: DeserializeOwned>(
        &self,
        endpoint: Endpoint<'_>,
        session: &mut Session,
    ) -> Result<Paged<W>, FetchError> {
        let first = endpoint.url(&self.repo);
        let mut url = first.clone();
        let mut items = Vec::new();
        let mut pages_fetched = 0u32;

        let stop = |items, pages_fetched, reason| {
            Ok(Paged {
                items,
                pages_fetched,
                completeness: Completeness::Partial(reason),
            })
        };

        loop {
            let page = match self.send(&url, endpoint.accept(), session).await {
                Ok(Body::Truncated { limit, .. }) => Err(FetchError::ResponseTooLarge { limit }),
                Ok(Body::Full(response)) => serde_json::from_slice::<Vec<W>>(&response.body)
                    .map(|parsed| (parsed, response))
                    .map_err(|_| FetchError::Malformed("unexpected JSON page shape")),
                Err(e) => Err(e),
            };
            let (parsed, response) = match page {
                Ok(page) => page,
                Err(e) if pages_fetched == 0 => return Err(e),
                Err(
                    FetchError::ResponseTooLarge { limit }
                    | FetchError::ByteBudgetExhausted { limit },
                ) => {
                    return stop(items, pages_fetched, PartialReason::ByteCap { limit });
                }
                Err(e) => return stop(items, pages_fetched, PartialReason::StoppedBy(e)),
            };
            items.extend(parsed);
            pages_fetched += 1;

            url = match next_link(&response, &first, pages_fetched) {
                None => {
                    return Ok(Paged {
                        items,
                        pages_fetched,
                        completeness: Completeness::Complete,
                    });
                }
                Some(Err(e)) => return stop(items, pages_fetched, PartialReason::StoppedBy(e)),
                Some(Ok(_)) if pages_fetched >= self.limits.max_pages => {
                    return stop(
                        items,
                        pages_fetched,
                        PartialReason::PageCap {
                            max_pages: self.limits.max_pages,
                        },
                    );
                }
                Some(Ok(next)) => next,
            };
        }
    }

    async fn send(
        &self,
        url: &Url,
        accept: &'static str,
        session: &mut Session,
    ) -> Result<Body, FetchError> {
        // The total budget is enforced before every read, so the last page cannot overrun it.
        let remaining = self
            .limits
            .max_total_bytes
            .saturating_sub(session.bytes_read);
        if remaining == 0 {
            return Err(FetchError::ByteBudgetExhausted {
                limit: self.limits.max_total_bytes,
            });
        }
        let (cap, cap_limit) = if remaining < self.limits.max_response_bytes {
            (remaining, self.limits.max_total_bytes)
        } else {
            (
                self.limits.max_response_bytes,
                self.limits.max_response_bytes,
            )
        };
        let bearer = match &self.credential {
            Credential::Anonymous => None,
            Credential::UserToken(token) => Some(token.clone()),
        };
        let request = HttpRequest {
            method: Method::Get,
            url: url.clone(),
            headers: vec![
                ("accept", accept.to_owned()),
                ("x-github-api-version", API_VERSION.to_owned()),
            ],
            bearer,
            form: Vec::new(),
        };
        let mut receipt = Receipt {
            method: Method::Get,
            host: url.host_str().unwrap_or_default().to_owned(),
            path_and_query: match url.query() {
                Some(q) => format!("{}?{q}", url.path()),
                None => url.path().to_owned(),
            },
            accept,
            api_version: API_VERSION,
            authenticated: request.bearer.is_some(),
            status: None,
            github_request_id: None,
            ratelimit_resource: None,
            ratelimit_remaining: None,
            body_bytes: 0,
            body_sha256: None,
            body_truncated: false,
            retrieved_at_unix_ms: 0,
            outcome: String::new(),
        };
        let outcome = match self.transport.send(&request, cap).await {
            Err(e) => Err(FetchError::Transport(e)),
            Ok(response) => {
                receipt.status = Some(response.status);
                receipt.retrieved_at_unix_ms = unix_ms_now();
                receipt.github_request_id =
                    response.header("x-github-request-id").map(str::to_owned);
                receipt.ratelimit_resource =
                    response.header("x-ratelimit-resource").map(str::to_owned);
                receipt.ratelimit_remaining =
                    response.header("x-ratelimit-remaining").map(str::to_owned);
                receipt.body_bytes = response.body.len();
                receipt.body_sha256 = Some(sha256_hex(&response.body));
                receipt.body_truncated = response.body_truncated;
                session.bytes_read += response.body.len();
                classify(&response).map(|()| {
                    if response.body_truncated {
                        Body::Truncated {
                            response,
                            limit: cap_limit,
                        }
                    } else {
                        Body::Full(response)
                    }
                })
            }
        };
        receipt.outcome = match &outcome {
            Ok(_) => "ok".to_owned(),
            Err(e) => e.to_string(),
        };
        session.receipts.push(receipt);
        outcome
    }
}

/// Mutable pull request endpoints are never revision-bound, so full coverage is reported
/// as `MutableSourceUnpinned`. A coverage limit already reported is kept.
fn unpinned(coverage: Completeness) -> Completeness {
    match coverage {
        Completeness::Complete => Completeness::Partial(PartialReason::MutableSourceUnpinned),
        partial @ Completeness::Partial(_) => partial,
    }
}

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Reads only the `page` number from `Link: rel="next"` and rebuilds the URL from our own
/// endpoint. GitHub's links point at `/repositories/{id}/...`, and a URL taken from a
/// response is never requested. `None` means no next page.
fn next_link(
    response: &HttpResponse,
    first: &Url,
    pages_fetched: u32,
) -> Option<Result<Url, FetchError>> {
    let header = response.header("link")?;
    let target = header.split(',').find_map(|part| {
        let (url_part, params) = part.split_once(';')?;
        params
            .split(';')
            .any(|p| p.trim() == "rel=\"next\"")
            .then(|| {
                url_part
                    .trim()
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .to_owned()
            })
    })?;
    let page = Url::parse(&target)
        .ok()
        .and_then(|url| {
            url.query_pairs()
                .find(|(k, _)| k == "page")
                .and_then(|(_, v)| v.parse::<u32>().ok())
        })
        .filter(|page| *page == pages_fetched + 1);
    Some(match page {
        Some(page) => {
            let mut next = first.clone();
            next.query_pairs_mut()
                .append_pair("page", &page.to_string());
            Ok(next)
        }
        None => Err(FetchError::PaginationUnrecognized),
    })
}

fn map_paged<W, D: From<W>>(paged: Paged<W>) -> Paged<D> {
    Paged {
        items: paged.items.into_iter().map(D::from).collect(),
        pages_fetched: paged.pages_fetched,
        completeness: paged.completeness,
    }
}

/// GitHub wire shapes. Only fields the crate uses; everything else is ignored.
mod wire {
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(tag = "type", rename_all = "lowercase")]
    pub enum ContentEntry {
        File {
            sha: String,
            size: u64,
            encoding: Option<String>,
            content: Option<String>,
        },
        Dir,
        Symlink,
        Submodule,
    }

    #[derive(Deserialize)]
    pub struct Issue {
        pub number: u64,
        pub title: String,
        pub state: String,
        pub body: Option<String>,
        pub comments: u64,
        pub updated_at: String,
        pub pull_request: Option<serde_json::Value>,
    }

    #[derive(Deserialize)]
    pub struct Comment {
        pub id: u64,
        pub author_association: String,
        pub body: Option<String>,
        pub created_at: String,
        pub updated_at: String,
    }

    #[derive(Deserialize)]
    pub struct GitRefSha {
        pub sha: String,
    }

    #[derive(Deserialize)]
    pub struct Pull {
        pub number: u64,
        pub title: String,
        pub state: String,
        pub merged: bool,
        pub base: GitRefSha,
        pub head: GitRefSha,
        pub changed_files: u64,
        pub body: Option<String>,
        pub updated_at: String,
    }

    #[derive(Deserialize)]
    pub struct Compare {
        /// Required: a response without it is malformed, never an empty complete list.
        pub files: Vec<PullFile>,
    }

    #[derive(Deserialize)]
    pub struct PullFile {
        pub filename: String,
        pub status: String,
        pub additions: u64,
        pub deletions: u64,
        pub patch: Option<String>,
    }

    #[derive(Deserialize)]
    pub struct CommitAuthor {
        pub date: Option<String>,
    }

    #[derive(Deserialize)]
    pub struct CommitInner {
        pub message: String,
        pub author: Option<CommitAuthor>,
    }

    #[derive(Deserialize)]
    pub struct Commit {
        pub sha: String,
        pub commit: CommitInner,
    }
}

impl From<wire::Issue> for IssueDoc {
    fn from(w: wire::Issue) -> Self {
        Self {
            number: w.number,
            title: w.title,
            state: w.state,
            is_pull_request: w.pull_request.is_some(),
            body: w.body,
            comments: w.comments,
            updated_at: w.updated_at,
        }
    }
}

impl From<wire::Comment> for CommentDoc {
    fn from(w: wire::Comment) -> Self {
        Self {
            id: w.id,
            author_association: w.author_association,
            body: w.body,
            created_at: w.created_at,
            updated_at: w.updated_at,
        }
    }
}

impl TryFrom<wire::Pull> for PullDoc {
    type Error = FetchError;

    fn try_from(w: wire::Pull) -> Result<Self, FetchError> {
        let sha =
            |s: &str| CommitSha::parse(s).map_err(|_| FetchError::Malformed("pull request sha"));
        Ok(Self {
            number: w.number,
            title: w.title,
            state: w.state,
            merged: w.merged,
            revisions: PullRevisions {
                base: sha(&w.base.sha)?,
                head: sha(&w.head.sha)?,
            },
            changed_files: w.changed_files,
            body: w.body,
            updated_at: w.updated_at,
        })
    }
}

impl From<wire::PullFile> for PullFileDoc {
    fn from(w: wire::PullFile) -> Self {
        Self {
            filename: w.filename,
            status: w.status,
            additions: w.additions,
            deletions: w.deletions,
            patch: w.patch,
        }
    }
}

impl From<wire::Commit> for CommitDoc {
    fn from(w: wire::Commit) -> Self {
        Self {
            sha: w.sha,
            message: w.commit.message,
            authored_at: w.commit.author.and_then(|a| a.date),
        }
    }
}
