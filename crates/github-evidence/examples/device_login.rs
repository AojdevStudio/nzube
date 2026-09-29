//! Live product-auth check: GitHub App device flow, then read-only evidence reads.
//!
//! The client ID is injected at runtime and never compiled in:
//!
//! ```sh
//! NZUBE_GITHUB_APP_CLIENT_ID=<client id> \
//!   cargo run -p github-evidence --example device_login -- <owner/repo> <branch> [issue] [pull]
//! ```
//!
//! Prints a redacted JSON report. Tokens stay in memory and are dropped at exit.

use std::process::ExitCode;

use github_evidence::auth::{ClientId, DeviceFlow};
use github_evidence::client::{Credential, Evidence, EvidenceClient, Fetched, Limits};
use github_evidence::request::{EvidenceRequest, GitRef, IssueNumber, PullNumber, RepoRef};
use github_evidence::transport::ReqwestTransport;
use serde_json::{Value, json};

const CLIENT_ID_ENV: &str = "NZUBE_GITHUB_APP_CLIENT_ID";

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&report).expect("report serializes")
            );
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn text<T, E: std::fmt::Display>(result: Result<T, E>) -> Result<T, String> {
    result.map_err(|e| e.to_string())
}

async fn run() -> Result<Value, String> {
    let client_id = std::env::var(CLIENT_ID_ENV)
        .map_err(|_| format!("set {CLIENT_ID_ENV} to the registered GitHub App's client ID (docs/github-app-setup.md)"))?;
    let client_id = text(ClientId::parse(&client_id))?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [repo, branch, rest @ ..] = args.as_slice() else {
        return Err("usage: device_login <owner/repo> <branch> [issue] [pull]".to_owned());
    };
    let repo = text(RepoRef::parse(repo))?;
    let git_ref = text(GitRef::parse(branch))?;

    let flow = DeviceFlow::new(text(ReqwestTransport::new())?, client_id);
    let authorization = text(flow.start().await)?;
    eprintln!(
        "Open {} and enter code {}",
        authorization.verification_uri, authorization.user_code
    );
    let grant = text(flow.wait_for_grant(&authorization).await)?;
    let credential = Credential::UserToken(grant.access_token.clone());

    let resolved = fetch(
        &repo,
        &credential,
        EvidenceRequest::ResolveRevision {
            git_ref: git_ref.clone(),
        },
    )
    .await?;
    let mut reads = Vec::new();
    if let Ok(Evidence::Revision { sha, .. }) = &resolved.result {
        reads.push(EvidenceRequest::InstructionsFile { at: sha.clone() });
        reads.push(EvidenceRequest::History {
            from: sha.clone(),
            path: None,
        });
    }
    if let Some(n) = rest.first() {
        let number = text(IssueNumber::parse(n))?;
        reads.extend([
            EvidenceRequest::Issue { number },
            EvidenceRequest::IssueComments { number },
        ]);
    }
    let mut results = vec![report(&resolved)];
    for request in reads {
        results.push(report(&fetch(&repo, &credential, request).await?));
    }
    if let Some(n) = rest.get(1) {
        let number = text(PullNumber::parse(n))?;
        let pull = fetch(&repo, &credential, EvidenceRequest::PullRequest { number }).await?;
        // Files and diff come from compare pinned to the base and head this read observed.
        let revisions = match &pull.result {
            Ok(Evidence::PullRequest(doc)) => Some(doc.revisions.clone()),
            _ => None,
        };
        results.push(report(&pull));
        if let Some(revisions) = revisions {
            let files = EvidenceRequest::CompareFiles {
                revisions: revisions.clone(),
            };
            results.push(report(&fetch(&repo, &credential, files).await?));
            let diff = EvidenceRequest::CompareDiff { revisions };
            results.push(report(&fetch(&repo, &credential, diff).await?));
        }
    }

    // Device-flow refresh carries no client secret. The pre-refresh token is then re-tried.
    let refresh = match &grant.refresh_token {
        None => json!("no refresh token issued (token expiration disabled for this app)"),
        Some(refresh_token) => match flow.refresh(refresh_token).await {
            Err(e) => json!({ "refresh_error": e.to_string() }),
            Ok(rotated) => {
                let probe = || EvidenceRequest::ResolveRevision {
                    git_ref: git_ref.clone(),
                };
                let new =
                    fetch(&repo, &Credential::UserToken(rotated.access_token), probe()).await?;
                let old = fetch(&repo, &credential, probe()).await?;
                json!({ "refreshed_token": outcome(&new), "pre_refresh_token": outcome(&old) })
            }
        },
    };

    Ok(json!({
        "repo": repo,
        "access_expires_in_secs": grant.access_expires_in.map(|d| d.as_secs()),
        "refresh_token_issued": grant.refresh_token.is_some(),
        "results": results,
        "refresh": refresh,
    }))
}

async fn fetch(
    repo: &RepoRef,
    credential: &Credential,
    request: EvidenceRequest,
) -> Result<Fetched, String> {
    let transport = text(ReqwestTransport::new())?;
    Ok(EvidenceClient::new(
        transport,
        repo.clone(),
        credential.clone(),
        Limits::default(),
    )
    .fetch(&request)
    .await)
}

fn outcome(fetched: &Fetched) -> String {
    match &fetched.result {
        Ok(_) => "ok".to_owned(),
        Err(e) => e.to_string(),
    }
}

/// Outcome plus redacted receipts; evidence content is not printed.
fn report(fetched: &Fetched) -> Value {
    json!({ "outcome": outcome(fetched), "receipts": fetched.receipts })
}
