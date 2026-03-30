use std::collections::BTreeMap;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use crate::models::{CiStatus, ReviewStatus};

#[derive(Debug, Clone)]
pub struct PullRequest {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub head_ref_name: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct GitHubClient {
    verbose: bool,
}

impl GitHubClient {
    pub fn new(verbose: bool) -> Self {
        Self { verbose }
    }

    pub fn list_open_pull_requests(&self) -> Result<Vec<PullRequest>> {
        let prs: Vec<SearchPullRequest> = run_json(
            self.verbose,
            "gh",
            &[
                "search",
                "prs",
                "--author=@me",
                "--state=open",
                "--json",
                "number,title,url,repository",
            ],
        )?;

        let mut resolved = Vec::with_capacity(prs.len());
        for pr in prs {
            let head_ref_name =
                self.pull_request_head_ref(&pr.repository.name_with_owner, pr.number)?;
            resolved.push(PullRequest {
                repo: pr.repository.name_with_owner,
                number: pr.number,
                title: pr.title,
                head_ref_name,
                url: pr.url,
            });
        }

        Ok(resolved)
    }

    pub fn pull_request_checks(&self, pr: &PullRequest) -> Result<CiStatus> {
        let checks: Vec<PullRequestCheck> = run_json_with_allowed_exit_codes(
            self.verbose,
            "gh",
            &[
                "pr",
                "checks",
                &pr.number.to_string(),
                "-R",
                &pr.repo,
                "--json",
                "bucket,state",
            ],
            &[0, 8],
        )?;

        if checks.is_empty() {
            return Ok(CiStatus::NoChecks);
        }

        if checks
            .iter()
            .any(|check| matches!(check.bucket.as_deref(), Some("fail" | "cancel")))
        {
            return Ok(CiStatus::Failed);
        }

        if checks
            .iter()
            .any(|check| matches!(check.bucket.as_deref(), Some("pending")))
        {
            return Ok(CiStatus::Pending);
        }

        if checks
            .iter()
            .all(|check| matches!(check.bucket.as_deref(), Some("pass" | "skipping")))
        {
            return Ok(CiStatus::Passed);
        }

        if checks.iter().any(|check| {
            matches!(
                check.state.as_deref(),
                Some("PENDING" | "IN_PROGRESS" | "QUEUED")
            )
        }) {
            return Ok(CiStatus::Pending);
        }

        Ok(CiStatus::Failed)
    }

    pub fn pull_request_review_status(&self, pr: &PullRequest) -> Result<ReviewStatus> {
        let reviews: Vec<PullRequestReview> = run_json(
            self.verbose,
            "gh",
            &[
                "api",
                &format!("repos/{}/pulls/{}/reviews", pr.repo, pr.number),
            ],
        )?;

        if reviews.is_empty() {
            return Ok(ReviewStatus::NoReviews);
        }

        let mut latest_by_reviewer = BTreeMap::new();
        for review in reviews {
            if let Some(login) = review.user.and_then(|user| user.login) {
                latest_by_reviewer.insert(login, review.state);
            }
        }

        if latest_by_reviewer.is_empty() {
            return Ok(ReviewStatus::Pending);
        }

        if latest_by_reviewer
            .values()
            .any(|state| matches!(state.as_deref(), Some("CHANGES_REQUESTED")))
        {
            return Ok(ReviewStatus::ChangesRequested);
        }

        if latest_by_reviewer
            .values()
            .any(|state| matches!(state.as_deref(), Some("APPROVED")))
        {
            return Ok(ReviewStatus::Approved);
        }

        Ok(ReviewStatus::Pending)
    }

    fn pull_request_head_ref(&self, repo: &str, number: u64) -> Result<String> {
        let details: PullRequestHead = run_json(
            self.verbose,
            "gh",
            &[
                "pr",
                "view",
                &number.to_string(),
                "-R",
                repo,
                "--json",
                "headRefName",
            ],
        )?;

        Ok(details.head_ref_name)
    }
}

#[derive(Debug, Deserialize)]
struct SearchPullRequest {
    number: u64,
    title: String,
    url: String,
    repository: SearchRepository,
}

#[derive(Debug, Deserialize)]
struct SearchRepository {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

#[derive(Debug, Deserialize)]
struct PullRequestHead {
    #[serde(rename = "headRefName")]
    head_ref_name: String,
}

#[derive(Debug, Deserialize)]
struct PullRequestCheck {
    #[serde(default)]
    bucket: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PullRequestReview {
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    user: Option<ReviewUser>,
}

#[derive(Debug, Deserialize)]
struct ReviewUser {
    #[serde(default)]
    login: Option<String>,
}

fn run_json<T>(verbose: bool, program: &str, args: &[&str]) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    run_json_with_allowed_exit_codes(verbose, program, args, &[0])
}

fn run_json_with_allowed_exit_codes<T>(
    verbose: bool,
    program: &str,
    args: &[&str],
    allowed_exit_codes: &[i32],
) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    if verbose {
        eprintln!("> {} {}", program, args.join(" "));
    }

    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("spawning `{program}`"))?;

    let exit_code = output.status.code().unwrap_or_default();
    if !allowed_exit_codes.contains(&exit_code) {
        return Err(anyhow!(
            "`{program}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    serde_json::from_slice(&output.stdout)
        .with_context(|| format!("parsing JSON output from `{program}`"))
}
