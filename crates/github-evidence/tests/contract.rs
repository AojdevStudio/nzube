//! Fixture-transport contract checks. Payloads here are original, not captured from GitHub.

use std::collections::VecDeque;
use std::sync::Mutex;

use github_evidence::auth::{AuthError, ClientId, DeviceFlow, PollOutcome};
use github_evidence::classify::{DenialReason, FetchError, RateLimitKind};
use github_evidence::client::{
    Completeness, Credential, Evidence, EvidenceClient, FileContent, Limits, PartialReason,
};
use github_evidence::request::*;
use github_evidence::secret::SecretToken;
use github_evidence::transport::{
    API_VERSION, FormValue, HttpRequest, HttpResponse, Method, Transport, TransportError,
};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const TOKEN: &str = "ghu_FIXTURE_SENTINEL_TOKEN_0000";

#[derive(Default)]
struct Fixture {
    replies: Mutex<VecDeque<Result<HttpResponse, TransportError>>>,
    seen: Mutex<Vec<HttpRequest>>,
}

impl Fixture {
    fn with(replies: impl IntoIterator<Item = Result<HttpResponse, TransportError>>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().collect()),
            seen: Mutex::default(),
        }
    }
    fn seen(&self) -> Vec<HttpRequest> {
        self.seen.lock().unwrap().clone()
    }
}

impl Transport for &Fixture {
    async fn send(
        &self,
        request: &HttpRequest,
        max_body_bytes: usize,
    ) -> Result<HttpResponse, TransportError> {
        self.seen.lock().unwrap().push(request.clone());
        let mut reply = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("fixture ran out of replies")?;
        if reply.body.len() > max_body_bytes {
            reply.body.truncate(max_body_bytes);
            reply.body_truncated = true;
        }
        Ok(reply)
    }
}

fn reply(
    status: u16,
    headers: &[(&str, &str)],
    body: &str,
) -> Result<HttpResponse, TransportError> {
    Ok(HttpResponse {
        status,
        headers: headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        body: body.as_bytes().to_vec(),
        body_truncated: false,
    })
}

fn repo() -> RepoRef {
    RepoRef::parse("fixture-org/fixture-repo").unwrap()
}

fn sha() -> CommitSha {
    CommitSha::parse(SHA).unwrap()
}

fn client(fixture: &Fixture, limits: Limits) -> EvidenceClient<&Fixture> {
    EvidenceClient::new(
        fixture,
        repo(),
        Credential::UserToken(SecretToken::new(TOKEN)),
        limits,
    )
}

fn file_json(text: &str) -> String {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    format!(
        r#"{{"type":"file","sha":"blobsha","size":{},"encoding":"base64","content":"{encoded}"}}"#,
        text.len()
    )
}

fn next(url: &str) -> (&'static str, String) {
    (
        "link",
        format!(r#"<{url}>; rel="next", <{url}>; rel="last""#),
    )
}

#[test]
fn endpoint_allowlist_is_exact() {
    let path = RepoPath::parse("src/main.rs").unwrap();
    let issue = IssueNumber::parse("7").unwrap();
    let pull = PullNumber::parse("#9").unwrap();
    let base = "https://api.github.com/repos/fixture-org/fixture-repo";
    let cases = [
        (
            Endpoint::ListCommits {
                sha: "main",
                path: None,
                per_page: 1,
            },
            format!("{base}/commits?sha=main&per_page=1"),
            ACCEPT_JSON,
        ),
        (
            Endpoint::ListCommits {
                sha: SHA,
                path: Some(&path),
                per_page: 30,
            },
            format!("{base}/commits?sha={SHA}&path=src%2Fmain.rs&per_page=30"),
            ACCEPT_JSON,
        ),
        (
            Endpoint::Contents {
                path: &path,
                at: &sha(),
            },
            format!("{base}/contents/src/main.rs?ref={SHA}"),
            ACCEPT_JSON,
        ),
        (
            Endpoint::Issue { number: issue },
            format!("{base}/issues/7"),
            ACCEPT_JSON,
        ),
        (
            Endpoint::IssueComments {
                number: issue,
                per_page: 50,
            },
            format!("{base}/issues/7/comments?per_page=50"),
            ACCEPT_JSON,
        ),
        (
            Endpoint::Pull { number: pull },
            format!("{base}/pulls/9"),
            ACCEPT_JSON,
        ),
        (
            Endpoint::PullFiles {
                number: pull,
                per_page: 100,
            },
            format!("{base}/pulls/9/files?per_page=100"),
            ACCEPT_JSON,
        ),
        (
            Endpoint::PullDiff { number: pull },
            format!("{base}/pulls/9"),
            ACCEPT_DIFF,
        ),
        (
            Endpoint::Compare { revisions: &revs() },
            format!("{base}/compare/{BASE_SHA}...{SHA}?per_page=1"),
            ACCEPT_JSON,
        ),
        (
            Endpoint::CompareDiff { revisions: &revs() },
            format!("{base}/compare/{BASE_SHA}...{SHA}"),
            ACCEPT_DIFF,
        ),
    ];
    for (endpoint, url, accept) in cases {
        assert_eq!(endpoint.url(&repo()).as_str(), url, "{endpoint:?}");
        assert_eq!(endpoint.accept(), accept, "{endpoint:?}");
    }
}

#[test]
fn repo_input_cannot_escape_owner_repo_or_host() {
    for ok in [
        "o/r",
        "https://github.com/o/r",
        "https://github.com/o/r.git",
        "github.com/o/r",
        "https://github.com/o/r/pull/3",
    ] {
        assert_eq!(RepoRef::parse(ok).unwrap().to_string(), "o/r", "{ok}");
    }
    let rejected = [
        ("https://evil.example/o/r", InputError::ForeignHost),
        (
            "https://github.com.evil.example/o/r",
            InputError::ForeignHost,
        ),
        ("https://api.github.com/o/r", InputError::ForeignHost),
        ("http://github.com/o/r", InputError::ForeignHost),
        ("https://user:pw@github.com/o/r", InputError::ForeignHost),
        ("https://github.com:8443/o/r", InputError::ForeignHost),
        ("o/r/extra", InputError::RepoShape),
        ("o", InputError::RepoShape),
        ("../r", InputError::Owner),
        ("o/..", InputError::RepoName),
        ("o/r?x=1", InputError::RepoName),
        ("o/r%2F..", InputError::RepoName),
        ("github.com.evil.example/r", InputError::Owner),
    ];
    for (input, expected) in rejected {
        assert_eq!(RepoRef::parse(input), Err(expected), "{input}");
    }
}

#[test]
fn file_paths_stay_inside_the_contents_path() {
    for bad in [
        "",
        "/etc/passwd",
        "a/../b",
        "./a",
        "a//b",
        "a\\b",
        "a/\nb",
        "..",
    ] {
        assert_eq!(RepoPath::parse(bad), Err(InputError::Path), "{bad:?}");
    }
    let tricky = RepoPath::parse("docs/a?b#c%2F..x").unwrap();
    let url = Endpoint::Contents {
        path: &tricky,
        at: &sha(),
    }
    .url(&repo());
    assert_eq!(url.host_str(), Some("api.github.com"));
    assert_eq!(
        url.path(),
        "/repos/fixture-org/fixture-repo/contents/docs/a%3Fb%23c%252F..x"
    );
    assert_eq!(url.query(), Some(&*format!("ref={SHA}")));
    assert_eq!(url.fragment(), None);
    assert_eq!(
        GitRef::parse("feature/x").map(|r| r.as_str().to_owned()),
        Ok("feature/x".to_owned())
    );
    for bad in ["-x", "a..b", "a b", "a:b", "a~1", "", "a/"] {
        assert_eq!(GitRef::parse(bad), Err(InputError::GitRef), "{bad:?}");
    }
    assert_eq!(CommitSha::parse("abc"), Err(InputError::CommitSha));
}

#[tokio::test]
async fn every_evidence_request_is_a_versioned_get_on_the_allowlist() {
    let path = RepoPath::parse("README.md").unwrap();
    let requests_and_replies = [
        (EvidenceRequest::ResolveRevision { git_ref: GitRef::parse("main").unwrap() }, format!(r#"[{{"sha":"{SHA}","commit":{{"message":"m","author":{{"date":"d"}}}}}}]"#)),
        (EvidenceRequest::File { path: path.clone(), at: sha() }, file_json("hello")),
        (EvidenceRequest::InstructionsFile { at: sha() }, file_json("# rules")),
        (EvidenceRequest::Issue { number: IssueNumber::parse("1").unwrap() }, r#"{"number":1,"title":"t","state":"open","body":null,"comments":0,"updated_at":"u"}"#.to_owned()),
        (EvidenceRequest::IssueComments { number: IssueNumber::parse("1").unwrap() }, "[]".to_owned()),
        (EvidenceRequest::PullRequest { number: PullNumber::parse("2").unwrap() }, r#"{"number":2,"title":"t","state":"closed","merged":true,"base":{"sha":"abcdefabcdefabcdefabcdefabcdefabcdefabcd"},"head":{"sha":"0123456789abcdef0123456789abcdef01234567"},"changed_files":1,"body":null,"updated_at":"u"}"#.to_owned()),
        (EvidenceRequest::CompareFiles { revisions: revs() }, compare_json(&[])),
        (EvidenceRequest::CompareDiff { revisions: revs() }, "diff --git a/x b/x\n".to_owned()),
        (EvidenceRequest::PullRequestFiles { number: PullNumber::parse("2").unwrap() }, "[]".to_owned()),
        (EvidenceRequest::PullRequestDiff { number: PullNumber::parse("2").unwrap() }, "diff --git a/x b/x\n".to_owned()),
        (EvidenceRequest::History { from: sha(), path: Some(path) }, "[]".to_owned()),
    ];
    let allowed = |p: &str| {
        let rest = p
            .strip_prefix("/repos/fixture-org/fixture-repo/")
            .unwrap_or("");
        let parts: Vec<&str> = rest.split('/').collect();
        let numeric = |s: &str| s.parse::<u64>().is_ok();
        match parts.as_slice() {
            ["commits"] => true,
            ["contents", ..] => parts.len() > 1,
            ["compare", range] => range.contains("..."),
            ["issues", n] | ["issues", n, "comments"] | ["pulls", n] | ["pulls", n, "files"] => {
                numeric(n)
            }
            _ => false,
        }
    };
    for (request, body) in requests_and_replies {
        // Files and diff re-read the pull request afterwards to check its head.
        let fixture = Fixture::with([reply(200, &[], &body), reply(200, &[], &pull_json(SHA, 0))]);
        let fetched = client(&fixture, Limits::default()).fetch(&request).await;
        assert!(fetched.result.is_ok(), "{request:?}: {:?}", fetched.result);
        for sent in fixture.seen() {
            assert_eq!(sent.method, Method::Get);
            assert_eq!(sent.url.scheme(), "https");
            assert_eq!(sent.url.host_str(), Some("api.github.com"));
            assert!(allowed(sent.url.path()), "not allowlisted: {}", sent.url);
            assert_eq!(sent.header("x-github-api-version"), Some(API_VERSION));
            assert!(sent.form.is_empty());
            assert!(sent.bearer.as_ref().is_some_and(|t| t.matches(TOKEN)));
        }
    }
}

/// Status, response headers, response body, expected outcome.
type StatusCase = (
    u16,
    Vec<(&'static str, &'static str)>,
    &'static str,
    FetchError,
);

#[tokio::test]
async fn status_classification_distinguishes_auth_permission_rate_limit_and_absence() {
    let cases: Vec<StatusCase> = vec![
        (
            401,
            vec![],
            r#"{"message":"Bad credentials"}"#,
            FetchError::InvalidCredentials,
        ),
        (
            403,
            vec![],
            r#"{"message":"Forbidden"}"#,
            FetchError::PermissionDenied {
                reason: DenialReason::Other,
            },
        ),
        (
            403,
            vec![(
                "x-github-sso",
                "required; url=https://github.com/orgs/o/sso?x",
            )],
            r#"{"message":"Resource protected by organization SAML enforcement."}"#,
            FetchError::PermissionDenied {
                reason: DenialReason::SsoRequired,
            },
        ),
        (
            403,
            vec![("x-ratelimit-remaining", "4000")],
            r#"{"message":"Resource not accessible by integration"}"#,
            FetchError::PermissionDenied {
                reason: DenialReason::NotGrantedToToken,
            },
        ),
        (
            403,
            vec![
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "1900000000"),
            ],
            r#"{"message":"API rate limit exceeded"}"#,
            FetchError::RateLimited {
                kind: RateLimitKind::Primary,
                retry_after_secs: None,
                reset_epoch: Some(1900000000),
            },
        ),
        (
            429,
            vec![
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "1900000000"),
            ],
            "{}",
            FetchError::RateLimited {
                kind: RateLimitKind::Primary,
                retry_after_secs: None,
                reset_epoch: Some(1900000000),
            },
        ),
        (
            429,
            vec![("retry-after", "60")],
            "{}",
            FetchError::RateLimited {
                kind: RateLimitKind::Secondary,
                retry_after_secs: Some(60),
                reset_epoch: None,
            },
        ),
        (
            403,
            vec![("x-ratelimit-remaining", "12")],
            r#"{"message":"You have exceeded a secondary rate limit."}"#,
            FetchError::RateLimited {
                kind: RateLimitKind::Secondary,
                retry_after_secs: None,
                reset_epoch: None,
            },
        ),
        (
            404,
            vec![],
            r#"{"message":"Not Found"}"#,
            FetchError::Unavailable,
        ),
        (
            301,
            vec![("location", "https://api.github.com/repositories/1")],
            "{}",
            FetchError::Moved { status: 301 },
        ),
        (
            410,
            vec![],
            r#"{"message":"Issues are disabled for this repo"}"#,
            FetchError::Gone,
        ),
        (
            502,
            vec![],
            "<html>",
            FetchError::UnexpectedStatus { status: 502 },
        ),
    ];
    for (status, headers, body, expected) in cases {
        let fixture = Fixture::with([reply(status, &headers, body)]);
        let fetched = client(&fixture, Limits::default())
            .fetch(&EvidenceRequest::Issue {
                number: IssueNumber::parse("1").unwrap(),
            })
            .await;
        assert_eq!(
            fetched.result.unwrap_err(),
            expected,
            "status {status} headers {headers:?}"
        );
    }
}

#[tokio::test]
async fn transport_and_malformed_responses_are_their_own_outcomes() {
    let issue = EvidenceRequest::Issue {
        number: IssueNumber::parse("1").unwrap(),
    };
    let fixture = Fixture::with([Err(TransportError::Timeout)]);
    assert_eq!(
        client(&fixture, Limits::default())
            .fetch(&issue)
            .await
            .result
            .unwrap_err(),
        FetchError::Transport(TransportError::Timeout)
    );
    let fixture = Fixture::with([reply(200, &[], "{\"number\":\"not a number\"}")]);
    assert!(matches!(
        client(&fixture, Limits::default())
            .fetch(&issue)
            .await
            .result,
        Err(FetchError::Malformed(_))
    ));
    let fixture = Fixture::with([reply(200, &[], r#"[{"type":"file"}]"#)]);
    let file = EvidenceRequest::File {
        path: RepoPath::parse("dir").unwrap(),
        at: sha(),
    };
    assert!(matches!(
        client(&fixture, Limits::default())
            .fetch(&file)
            .await
            .result,
        Err(FetchError::Malformed(_))
    ));
}

fn comments_page(ids: &[u64]) -> String {
    let items: Vec<String> = ids
        .iter()
        .map(|id| format!(r#"{{"id":{id},"author_association":"NONE","body":"c{id}","created_at":"t","updated_at":"t"}}"#))
        .collect();
    format!("[{}]", items.join(","))
}

const PAGE2: &str =
    "https://api.github.com/repos/fixture-org/fixture-repo/issues/5/comments?per_page=2&page=2";

const PAGE3: &str =
    "https://api.github.com/repos/fixture-org/fixture-repo/issues/5/comments?per_page=2&page=3";

#[tokio::test]
async fn page_cap_returns_partial_with_reason() {
    let fixture = Fixture::with([
        reply(
            200,
            &[(next(PAGE2).0, &next(PAGE2).1)],
            &comments_page(&[1, 2]),
        ),
        reply(
            200,
            &[(next(PAGE3).0, &next(PAGE3).1)],
            &comments_page(&[3, 4]),
        ),
    ]);
    let limits = Limits {
        per_page: 2,
        max_pages: 2,
        ..Limits::default()
    };
    let fetched = client(&fixture, limits)
        .fetch(&EvidenceRequest::IssueComments {
            number: IssueNumber::parse("5").unwrap(),
        })
        .await;
    let Ok(Evidence::IssueComments(paged)) = fetched.result else {
        panic!("{:?}", fetched.result)
    };
    assert_eq!(
        paged.items.iter().map(|c| c.id).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert_eq!(
        paged.completeness,
        Completeness::Partial(PartialReason::PageCap { max_pages: 2 })
    );
    assert_eq!(fixture.seen().len(), 2);
}

#[tokio::test]
async fn rate_limit_mid_pagination_keeps_earlier_pages_and_says_why() {
    let fixture = Fixture::with([
        reply(
            200,
            &[(next(PAGE2).0, &next(PAGE2).1)],
            &comments_page(&[1, 2]),
        ),
        reply(429, &[("retry-after", "30")], "{}"),
    ]);
    let limits = Limits {
        per_page: 2,
        ..Limits::default()
    };
    let fetched = client(&fixture, limits)
        .fetch(&EvidenceRequest::IssueComments {
            number: IssueNumber::parse("5").unwrap(),
        })
        .await;
    let Ok(Evidence::IssueComments(paged)) = fetched.result else {
        panic!("{:?}", fetched.result)
    };
    assert_eq!(paged.items.len(), 2);
    assert_eq!(
        paged.completeness,
        Completeness::Partial(PartialReason::StoppedBy(FetchError::RateLimited {
            kind: RateLimitKind::Secondary,
            retry_after_secs: Some(30),
            reset_epoch: None
        }))
    );
}

/// GitHub's next links use `/repositories/{numeric id}/...` rather than `/repos/{owner}/{repo}/...`.
#[tokio::test]
async fn pagination_follows_github_repository_id_links_by_page_number() {
    let github_next =
        "https://api.github.com/repositories/4242/issues/5/comments?per_page=2&page=2";
    let fixture = Fixture::with([
        reply(
            200,
            &[(next(github_next).0, &next(github_next).1)],
            &comments_page(&[1, 2]),
        ),
        reply(200, &[], &comments_page(&[3])),
    ]);
    let limits = Limits {
        per_page: 2,
        ..Limits::default()
    };
    let fetched = client(&fixture, limits)
        .fetch(&EvidenceRequest::IssueComments {
            number: IssueNumber::parse("5").unwrap(),
        })
        .await;
    let Ok(Evidence::IssueComments(paged)) = fetched.result else {
        panic!("{:?}", fetched.result)
    };
    assert_eq!(paged.completeness, Completeness::Complete);
    assert_eq!(paged.items.len(), 3);
    assert_eq!(fixture.seen()[1].url.as_str(), PAGE2);
}

#[tokio::test]
async fn pagination_never_requests_a_url_taken_from_a_response() {
    for (link, expected) in [
        ("https://evil.example/steal?page=2", None),
        (
            "https://api.github.com/repos/other/repo/issues/5/comments?page=2",
            None,
        ),
        (
            "https://api.github.com/repositories/1/issues/5/comments?after=cursor",
            Some(FetchError::PaginationUnrecognized),
        ),
        (
            "https://api.github.com/repositories/1/issues/5/comments?page=9",
            Some(FetchError::PaginationUnrecognized),
        ),
    ] {
        let fixture = Fixture::with([
            reply(200, &[(next(link).0, &next(link).1)], &comments_page(&[1])),
            reply(200, &[], "[]"),
        ]);
        let fetched = client(&fixture, Limits::default())
            .fetch(&EvidenceRequest::IssueComments {
                number: IssueNumber::parse("5").unwrap(),
            })
            .await;
        let Ok(Evidence::IssueComments(paged)) = fetched.result else {
            panic!("{:?}", fetched.result)
        };
        match expected {
            None => assert_eq!(paged.completeness, Completeness::Complete, "{link}"),
            Some(e) => assert_eq!(
                paged.completeness,
                Completeness::Partial(PartialReason::StoppedBy(e)),
                "{link}"
            ),
        }
        for sent in fixture.seen() {
            assert_eq!(sent.url.host_str(), Some("api.github.com"), "{link}");
            assert!(
                sent.url
                    .path()
                    .starts_with("/repos/fixture-org/fixture-repo/issues/5/comments"),
                "{link} -> {}",
                sent.url
            );
        }
    }
}

#[tokio::test]
async fn byte_caps_mark_diff_partial_and_refuse_truncated_json() {
    let limits = Limits {
        max_response_bytes: 16,
        ..Limits::default()
    };
    let fixture = Fixture::with([
        reply(
            200,
            &[],
            "diff --git a/long b/long\n+++ more than sixteen bytes",
        ),
        reply(200, &[], &pull_json(SHA, 1)),
    ]);
    let fetched = client(&fixture, limits)
        .fetch(&EvidenceRequest::CompareDiff { revisions: revs() })
        .await;
    let Ok(Evidence::CompareDiff {
        text, completeness, ..
    }) = fetched.result
    else {
        panic!("{:?}", fetched.result)
    };
    assert_eq!(text.len(), 16);
    assert_eq!(
        completeness,
        Completeness::Partial(PartialReason::ByteCap { limit: 16 })
    );

    let fixture = Fixture::with([reply(
        200,
        &[],
        r#"{"number":1,"title":"a long title","state":"open"}"#,
    )]);
    let fetched = client(&fixture, limits)
        .fetch(&EvidenceRequest::Issue {
            number: IssueNumber::parse("1").unwrap(),
        })
        .await;
    assert_eq!(
        fetched.result.unwrap_err(),
        FetchError::ResponseTooLarge { limit: 16 }
    );
}

#[tokio::test]
async fn instructions_report_absent_candidates_without_claiming_nonexistence() {
    let fixture = Fixture::with([
        reply(404, &[], r#"{"message":"Not Found"}"#),
        reply(200, &[], &file_json("# Claude rules")),
    ]);
    let fetched = client(&fixture, Limits::default())
        .fetch(&EvidenceRequest::InstructionsFile { at: sha() })
        .await;
    let Ok(Evidence::Instructions {
        found: Some(found),
        absent,
        ..
    }) = fetched.result
    else {
        panic!("{:?}", fetched.result)
    };
    assert_eq!(found.path.as_str(), "CLAUDE.md");
    assert!(matches!(found.content, FileContent::Text(ref t) if t == "# Claude rules"));
    assert_eq!(absent, [RepoPath::parse("AGENTS.md").unwrap()]);
    assert_eq!(fetched.receipts.len(), 2);
    assert!(
        fetched
            .receipts
            .iter()
            .all(|r| r.path_and_query.ends_with(&format!("?ref={SHA}")))
    );
}

#[tokio::test]
async fn tokens_and_bodies_never_reach_errors_or_receipts() {
    let body_marker = "BODY_ONLY_MARKER";
    let fixture = Fixture::with([reply(
        403,
        &[],
        &format!(r#"{{"message":"{body_marker}"}}"#),
    )]);
    let fetched = client(&fixture, Limits::default())
        .fetch(&EvidenceRequest::Issue {
            number: IssueNumber::parse("1").unwrap(),
        })
        .await;
    let rendered = format!(
        "{:?}\n{}\n{}",
        fetched,
        fetched.result.as_ref().unwrap_err(),
        serde_json::to_string(&fetched.receipts).unwrap()
    );
    assert!(!rendered.contains(TOKEN), "token leaked: {rendered}");
    assert!(!rendered.contains(body_marker), "body leaked: {rendered}");
    let sent = &fixture.seen()[0];
    assert!(!format!("{sent:?}").contains(TOKEN));
    assert!(sent.bearer.as_ref().is_some_and(|t| t.matches(TOKEN)));
}

#[tokio::test]
async fn anonymous_credential_sends_no_authorization() {
    let fixture = Fixture::with([reply(200, &[], "[]")]);
    let client = EvidenceClient::new(&fixture, repo(), Credential::Anonymous, Limits::default());
    client
        .fetch(&EvidenceRequest::IssueComments {
            number: IssueNumber::parse("1").unwrap(),
        })
        .await;
    assert!(fixture.seen()[0].bearer.is_none());
}

fn form_keys(request: &HttpRequest) -> Vec<&'static str> {
    request.form.iter().map(|(k, _)| *k).collect()
}

#[tokio::test]
async fn device_flow_is_secretless_and_separate_from_evidence() {
    let fixture = Fixture::with([
        reply(
            200,
            &[],
            r#"{"device_code":"dc_FIXTURE","user_code":"ABCD-EFGH","verification_uri":"https://github.com/login/device","expires_in":900,"interval":5}"#,
        ),
        reply(200, &[], r#"{"error":"authorization_pending"}"#),
        reply(200, &[], r#"{"error":"slow_down","interval":10}"#),
        reply(
            200,
            &[],
            &format!(
                r#"{{"access_token":"{TOKEN}","expires_in":28800,"refresh_token":"ghr_FIXTURE_REFRESH","refresh_token_expires_in":15897600,"scope":"","token_type":"bearer"}}"#
            ),
        ),
        reply(
            200,
            &[],
            &format!(
                r#"{{"access_token":"{TOKEN}","expires_in":28800,"refresh_token":"ghr_FIXTURE_REFRESH2","refresh_token_expires_in":15897600,"scope":"","token_type":"bearer"}}"#
            ),
        ),
    ]);
    let flow = DeviceFlow::new(&fixture, ClientId::parse("Iv23liFIXTURE000").unwrap());
    let authorization = flow.start().await.unwrap();
    assert_eq!(authorization.user_code, "ABCD-EFGH");
    assert!(matches!(
        flow.poll(&authorization.device_code).await,
        Ok(PollOutcome::Pending)
    ));
    assert!(
        matches!(flow.poll(&authorization.device_code).await, Ok(PollOutcome::SlowDown { interval }) if interval.as_secs() == 10)
    );
    let Ok(PollOutcome::Granted(grant)) = flow.poll(&authorization.device_code).await else {
        panic!()
    };
    assert!(grant.access_token.matches(TOKEN));
    assert!(!format!("{grant:?}").contains(TOKEN));
    let refreshed = flow
        .refresh(grant.refresh_token.as_ref().unwrap())
        .await
        .unwrap();
    assert!(
        refreshed
            .refresh_token
            .unwrap()
            .matches("ghr_FIXTURE_REFRESH2")
    );

    let seen = fixture.seen();
    assert_eq!(
        seen.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(),
        [
            "https://github.com/login/device/code",
            "https://github.com/login/oauth/access_token",
            "https://github.com/login/oauth/access_token",
            "https://github.com/login/oauth/access_token",
            "https://github.com/login/oauth/access_token",
        ]
    );
    assert!(
        seen.iter()
            .all(|r| r.method == Method::Post && r.bearer.is_none())
    );
    assert_eq!(form_keys(&seen[0]), ["client_id"]);
    assert_eq!(
        form_keys(&seen[1]),
        ["client_id", "device_code", "grant_type"]
    );
    assert_eq!(
        form_keys(&seen[4]),
        ["client_id", "grant_type", "refresh_token"]
    );
    assert!(
        seen.iter()
            .flat_map(|r| &r.form)
            .all(|(k, _)| *k != "client_secret")
    );
    assert!(matches!(&seen[4].form[2].1, FormValue::Secret(s) if s.matches("ghr_FIXTURE_REFRESH")));
}

#[tokio::test]
async fn device_flow_rejects_non_app_tokens_and_reports_setup_errors() {
    let fixture = Fixture::with([
        reply(
            200,
            &[],
            r#"{"access_token":"gho_OAUTH_APP_TOKEN","scope":"repo","token_type":"bearer"}"#,
        ),
        reply(200, &[], r#"{"error":"device_flow_disabled"}"#),
        reply(200, &[], r#"{"error":"access_denied"}"#),
    ]);
    let flow = DeviceFlow::new(&fixture, ClientId::parse("Iv23liFIXTURE000").unwrap());
    let code = SecretToken::new("dc");
    assert_eq!(
        flow.poll(&code).await.unwrap_err(),
        AuthError::NotAppUserToken
    );
    assert_eq!(
        flow.start().await.unwrap_err(),
        AuthError::DeviceFlowDisabled
    );
    assert_eq!(flow.poll(&code).await.unwrap_err(), AuthError::AccessDenied);
    assert_eq!(ClientId::parse(""), Err(AuthError::InvalidClientId));
    assert_eq!(
        ClientId::parse("has space in it"),
        Err(AuthError::InvalidClientId)
    );
}

#[tokio::test]
async fn total_byte_budget_bounds_the_last_page_too() {
    let first = comments_page(&[1]);
    let last = comments_page(&[2, 3, 4]);
    let budget = first.len() + 5;
    let fixture = Fixture::with([
        reply(200, &[(next(PAGE2).0, &next(PAGE2).1)], &first),
        reply(200, &[], &last),
    ]);
    let limits = Limits {
        per_page: 2,
        max_total_bytes: budget,
        ..Limits::default()
    };
    let fetched = client(&fixture, limits)
        .fetch(&EvidenceRequest::IssueComments {
            number: IssueNumber::parse("5").unwrap(),
        })
        .await;
    let Ok(Evidence::IssueComments(paged)) = fetched.result else {
        panic!("{:?}", fetched.result)
    };
    assert_eq!(paged.items.len(), 1);
    assert_eq!(
        paged.completeness,
        Completeness::Partial(PartialReason::ByteCap { limit: budget })
    );
    let read: usize = fetched.receipts.iter().map(|r| r.body_bytes).sum();
    assert!(
        read <= budget,
        "read {read} bytes over a {budget}-byte budget"
    );
}

#[tokio::test]
async fn zero_limits_are_rejected_before_any_request() {
    for limits in [
        Limits {
            max_pages: 0,
            ..Limits::default()
        },
        Limits {
            per_page: 0,
            ..Limits::default()
        },
        Limits {
            max_response_bytes: 0,
            ..Limits::default()
        },
        Limits {
            max_total_bytes: 0,
            ..Limits::default()
        },
    ] {
        let fixture = Fixture::with([reply(200, &[], "[]")]);
        let fetched = client(&fixture, limits)
            .fetch(&EvidenceRequest::IssueComments {
                number: IssueNumber::parse("5").unwrap(),
            })
            .await;
        assert!(fetched.result.is_err(), "{limits:?}: {:?}", fetched.result);
        assert!(fixture.seen().is_empty(), "{limits:?} sent a request");
    }
}

fn pull_json(head: &str, changed_files: u64) -> String {
    format!(
        r#"{{"number":9,"title":"t","state":"open","merged":false,"base":{{"sha":"{BASE_SHA}"}},"head":{{"sha":"{head}"}},"changed_files":{changed_files},"body":null,"updated_at":"u"}}"#
    )
}

const BASE_SHA: &str = "abcdefabcdefabcdefabcdefabcdefabcdefabcd";

fn revs() -> PullRevisions {
    PullRevisions {
        base: CommitSha::parse(BASE_SHA).unwrap(),
        head: sha(),
    }
}

const OTHER_SHA: &str = "fedcba9876543210fedcba9876543210fedcba98";

fn files_page(names: &[&str]) -> String {
    let items: Vec<String> = names
        .iter()
        .map(|n| format!(r#"{{"filename":"{n}","status":"modified","additions":1,"deletions":0,"patch":"@@"}}"#))
        .collect();
    format!("[{}]", items.join(","))
}

#[tokio::test]
async fn pull_files_and_diff_are_read_from_compare_pinned_to_their_revisions() {
    let pinned = format!("/repos/fixture-org/fixture-repo/compare/{BASE_SHA}...{SHA}");
    let fixture = Fixture::with([reply(200, &[], &compare_json(&["a", "b"]))]);
    let fetched = client(&fixture, Limits::default())
        .fetch(&EvidenceRequest::CompareFiles { revisions: revs() })
        .await;
    let Ok(Evidence::CompareFiles { revisions, files }) = fetched.result else {
        panic!("{:?}", fetched.result)
    };
    assert_eq!(revisions, revs());
    assert_eq!(files.completeness, Completeness::Complete);
    assert_eq!(
        files
            .items
            .iter()
            .map(|f| f.filename.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    let seen = fixture.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].url.path(), pinned);
    assert_eq!(seen[0].url.query(), Some("per_page=1"));

    let fixture = Fixture::with([reply(200, &[], "diff --git a/a b/a\n")]);
    let fetched = client(&fixture, Limits::default())
        .fetch(&EvidenceRequest::CompareDiff { revisions: revs() })
        .await;
    let Ok(Evidence::CompareDiff {
        revisions,
        completeness,
        ..
    }) = fetched.result
    else {
        panic!("{:?}", fetched.result)
    };
    assert_eq!(
        (revisions, completeness),
        (
            revs(),
            Completeness::Partial(PartialReason::DiffMayBeServerLimited)
        )
    );
    let seen = fixture.seen();
    assert_eq!(
        (seen.len(), seen[0].url.path(), seen[0].header("accept")),
        (1, pinned.as_str(), Some(ACCEPT_DIFF))
    );
}

#[tokio::test]
async fn compare_file_limit_is_reported_as_partial() {
    for (count, expected) in [
        (299, Completeness::Complete),
        (
            300,
            Completeness::Partial(PartialReason::CompareFileLimit { listed: 300 }),
        ),
    ] {
        let names: Vec<String> = (0..count).map(|i| format!("f{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let fixture = Fixture::with([reply(200, &[], &compare_json(&refs))]);
        let fetched = client(&fixture, Limits::default())
            .fetch(&EvidenceRequest::CompareFiles { revisions: revs() })
            .await;
        let Ok(Evidence::CompareFiles { files, .. }) = fetched.result else {
            panic!()
        };
        assert_eq!((files.items.len(), files.completeness), (count, expected));
    }
}

#[tokio::test]
async fn receipts_record_when_each_page_was_read() {
    let fixture = Fixture::with([
        reply(
            200,
            &[(next(PAGE2).0, &next(PAGE2).1)],
            &comments_page(&[1, 2]),
        ),
        reply(200, &[], &comments_page(&[3])),
    ]);
    let limits = Limits {
        per_page: 2,
        ..Limits::default()
    };
    let fetched = client(&fixture, limits)
        .fetch(&EvidenceRequest::IssueComments {
            number: IssueNumber::parse("5").unwrap(),
        })
        .await;
    let times: Vec<u64> = fetched
        .receipts
        .iter()
        .map(|r| r.retrieved_at_unix_ms)
        .collect();
    assert_eq!(times.len(), 2);
    assert!(
        times[0] > 1_700_000_000_000 && times[0] <= times[1],
        "{times:?}"
    );
    assert!(
        fetched
            .receipts
            .iter()
            .all(|r| r.body_sha256.as_ref().is_some_and(|h| h.len() == 64))
    );
}

fn compare_json(names: &[&str]) -> String {
    format!(
        r#"{{"status":"ahead","commits":[],"files":{}}}"#,
        files_page(names)
    )
}

#[allow(dead_code)]
fn pull_json_with(base: &str, head: &str, changed_files: u64) -> String {
    pull_json(head, changed_files).replace(BASE_SHA, base)
}

#[tokio::test]
async fn requested_revisions_decide_the_compare_range() {
    // A caller holding an older base asks for exactly that range; nothing re-reads the
    // pull request, so the result describes the requested SHAs whatever the PR shows now.
    let older = PullRevisions {
        base: CommitSha::parse(OTHER_SHA).unwrap(),
        head: sha(),
    };
    let fixture = Fixture::with([reply(200, &[], &compare_json(&["a"]))]);
    let fetched = client(&fixture, Limits::default())
        .fetch(&EvidenceRequest::CompareFiles {
            revisions: older.clone(),
        })
        .await;
    let Ok(Evidence::CompareFiles { revisions, .. }) = fetched.result else {
        panic!()
    };
    assert_eq!(revisions, older);
    assert_eq!(
        fixture.seen()[0].url.path(),
        format!("/repos/fixture-org/fixture-repo/compare/{OTHER_SHA}...{SHA}")
    );
}

#[tokio::test]
async fn inaccessible_compare_is_an_error_never_complete() {
    for (status, expected) in [
        (404, FetchError::Unavailable),
        (401, FetchError::InvalidCredentials),
        (
            403,
            FetchError::PermissionDenied {
                reason: DenialReason::Other,
            },
        ),
    ] {
        for request in [
            EvidenceRequest::CompareFiles { revisions: revs() },
            EvidenceRequest::CompareDiff { revisions: revs() },
        ] {
            let fixture = Fixture::with([reply(status, &[], "{}")]);
            let fetched = client(&fixture, Limits::default()).fetch(&request).await;
            assert_eq!(
                fetched.result.unwrap_err(),
                expected,
                "{status} {request:?}"
            );
        }
    }
}

fn flow(fixture: &Fixture) -> DeviceFlow<&Fixture> {
    DeviceFlow::new(fixture, ClientId::parse("Iv23liFIXTURE000").unwrap())
}

#[tokio::test(start_paused = true)]
async fn device_code_expiry_ends_polling_without_a_grant() {
    let fixture =
        Fixture::with((0..4).map(|_| reply(200, &[], r#"{"error":"authorization_pending"}"#)));
    let authorization = nzube_device_authorization(12, 5);
    assert_eq!(
        flow(&fixture)
            .wait_for_grant(&authorization)
            .await
            .unwrap_err(),
        AuthError::Expired
    );
    assert_eq!(
        fixture.seen().len(),
        2,
        "polled at 5s and 10s, stopped at the 12s expiry"
    );
}

#[tokio::test]
async fn auth_setup_and_expiry_errors_are_distinct() {
    let fixture = Fixture::with([
        reply(200, &[], r#"{"error":"incorrect_client_credentials"}"#),
        reply(200, &[], r#"{"error":"expired_token"}"#),
        reply(200, &[], r#"{"error":"bad_refresh_token"}"#),
        reply(401, &[], "{}"),
        reply(503, &[], "{}"),
        Err(TransportError::Connect),
    ]);
    let code = SecretToken::new("dc_FIXTURE");
    let refresh = SecretToken::new("ghr_FIXTURE_EXPIRED");
    assert_eq!(
        flow(&fixture).start().await.unwrap_err(),
        AuthError::IncorrectClientCredentials
    );
    assert_eq!(
        flow(&fixture).poll(&code).await.unwrap_err(),
        AuthError::Expired
    );
    assert_eq!(
        flow(&fixture).refresh(&refresh).await.unwrap_err(),
        AuthError::UnknownOAuthError
    );
    assert_eq!(
        flow(&fixture).refresh(&refresh).await.unwrap_err(),
        AuthError::Status(401)
    );
    assert_eq!(
        flow(&fixture).start().await.unwrap_err(),
        AuthError::Status(503)
    );
    assert_eq!(
        flow(&fixture).start().await.unwrap_err(),
        AuthError::Transport
    );
    let rendered: String = fixture.seen().iter().map(|r| format!("{r:?}")).collect();
    assert!(!rendered.contains("dc_FIXTURE") && !rendered.contains("ghr_FIXTURE_EXPIRED"));
}

/// Expired or revoked user tokens surface as 401 on reads; after a local disconnect the
/// client holds no credential and sends no Authorization header.
#[tokio::test]
async fn expired_token_reads_fail_and_disconnect_sends_no_credential() {
    let issue = EvidenceRequest::Issue {
        number: IssueNumber::parse("1").unwrap(),
    };
    let fixture = Fixture::with([reply(401, &[], r#"{"message":"Bad credentials"}"#)]);
    assert_eq!(
        client(&fixture, Limits::default())
            .fetch(&issue)
            .await
            .result
            .unwrap_err(),
        FetchError::InvalidCredentials
    );

    let fixture = Fixture::with([reply(404, &[], "{}")]);
    let disconnected =
        EvidenceClient::new(&fixture, repo(), Credential::Anonymous, Limits::default());
    assert_eq!(
        disconnected.fetch(&issue).await.result.unwrap_err(),
        FetchError::Unavailable
    );
    assert!(fixture.seen()[0].bearer.is_none());
}

fn nzube_device_authorization(
    expires_in: u64,
    interval: u64,
) -> github_evidence::auth::DeviceAuthorization {
    github_evidence::auth::DeviceAuthorization {
        device_code: SecretToken::new("dc_FIXTURE"),
        user_code: "ABCD-EFGH".to_owned(),
        verification_uri: "https://github.com/login/device".to_owned(),
        expires_in: std::time::Duration::from_secs(expires_in),
        interval: std::time::Duration::from_secs(interval),
    }
}

/// A base or head that moves A→B→A between reads must never yield content labeled with A
/// unless the request that fetched it was pinned to A's SHAs.
#[tokio::test]
async fn aba_revision_change_never_stamps_content_from_another_revision() {
    // Mutable source: content served while the head was at B. No re-read can make it
    // revision-bound, so it carries no revision label and is never Complete.
    let pull = PullNumber::parse("9").unwrap();
    let fixture = Fixture::with([
        reply(200, &[], &files_page(&["only-at-b.rs"])),
        reply(200, &[], "diff --git a/only-at-b.rs b/only-at-b.rs\n"),
    ]);
    let mutable = client(&fixture, Limits::default());
    let Ok(Evidence::PullRequestFiles(files)) = mutable
        .fetch(&EvidenceRequest::PullRequestFiles { number: pull })
        .await
        .result
    else {
        panic!()
    };
    assert_eq!(
        files.completeness,
        Completeness::Partial(PartialReason::MutableSourceUnpinned)
    );
    let Ok(Evidence::PullRequestDiff { completeness, .. }) = mutable
        .fetch(&EvidenceRequest::PullRequestDiff { number: pull })
        .await
        .result
    else {
        panic!()
    };
    assert_eq!(
        completeness,
        Completeness::Partial(PartialReason::MutableSourceUnpinned)
    );

    // Pinned source: the only way to get content labeled with A is a request for A.
    let pinned = format!("/repos/fixture-org/fixture-repo/compare/{BASE_SHA}...{SHA}");
    for request in [
        EvidenceRequest::CompareFiles { revisions: revs() },
        EvidenceRequest::CompareDiff { revisions: revs() },
    ] {
        let body = match request {
            EvidenceRequest::CompareFiles { .. } => compare_json(&["at-a.rs"]),
            _ => "diff --git a/at-a.rs b/at-a.rs\n".to_owned(),
        };
        let fixture = Fixture::with([reply(200, &[], &body)]);
        let fetched = client(&fixture, Limits::default()).fetch(&request).await;
        let stamped = match fetched.result {
            Ok(
                Evidence::CompareFiles { revisions, .. } | Evidence::CompareDiff { revisions, .. },
            ) => revisions,
            other => panic!("{other:?}"),
        };
        assert_eq!(stamped, revs());
        assert!(
            fetched
                .receipts
                .iter()
                .all(|r| r.path_and_query.starts_with(&pinned)),
            "{request:?}"
        );
    }
}

#[tokio::test]
async fn compare_response_without_files_is_malformed_never_complete() {
    let fixture = Fixture::with([reply(200, &[], r#"{"status":"ahead","commits":[]}"#)]);
    let fetched = client(&fixture, Limits::default())
        .fetch(&EvidenceRequest::CompareFiles { revisions: revs() })
        .await;
    assert!(
        matches!(fetched.result, Err(FetchError::Malformed(_))),
        "{:?}",
        fetched.result
    );
}

/// GitHub may drop parts of a rendered diff beyond its documented limits (300 files, 1 MB
/// total, 500 KB or 20,000 lines per file) without marking the cut, so a diff that fits the
/// local byte cap still cannot be proven complete.
#[tokio::test]
async fn compare_diff_is_never_claimed_complete() {
    let fixture = Fixture::with([reply(200, &[], "diff --git a/a b/a\n+small\n")]);
    let fetched = client(&fixture, Limits::default())
        .fetch(&EvidenceRequest::CompareDiff { revisions: revs() })
        .await;
    let Ok(Evidence::CompareDiff { completeness, .. }) = fetched.result else {
        panic!("{:?}", fetched.result)
    };
    assert_eq!(
        completeness,
        Completeness::Partial(PartialReason::DiffMayBeServerLimited),
        "a server-limited diff was labeled complete"
    );
}
