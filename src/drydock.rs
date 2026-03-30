use anyhow::{Context, Result};
use reqwest::Url;
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::models::{DrydockRun, ItemUpdate, PauseFlag, RunUpdate};

#[derive(Debug, Deserialize)]
struct DrydockDataResponse<T> {
    data: T,
}

#[derive(Debug, Clone)]
pub struct DrydockClient {
    client: Client,
    base_url: Url,
}

impl DrydockClient {
    pub fn new(api_url: &str) -> Result<Self> {
        let mut base_url =
            Url::parse(api_url).with_context(|| format!("invalid Drydock URL {api_url}"))?;
        let normalized_path = match base_url.path() {
            "" | "/" => "/".to_string(),
            path if path.ends_with('/') => path.to_string(),
            path => format!("{path}/"),
        };
        base_url.set_path(&normalized_path);
        let client = Client::builder()
            .build()
            .context("building Drydock HTTP client")?;
        Ok(Self { client, base_url })
    }

    pub fn list_runs(
        &self,
        status: Option<&str>,
        repo: Option<&str>,
        branch: Option<&str>,
    ) -> Result<Vec<DrydockRun>> {
        let mut url = self.endpoint("runs")?;
        {
            let mut query = url.query_pairs_mut();
            if let Some(status) = status {
                query.append_pair("status", status);
            }
            if let Some(repo) = repo {
                query.append_pair("repo", repo);
            }
            if let Some(branch) = branch {
                query.append_pair("branch", branch);
            }
        }

        let response: DrydockDataResponse<Vec<DrydockRun>> = self
            .client
            .get(url)
            .send()
            .context("requesting Drydock runs")?
            .error_for_status()
            .context("Drydock runs request failed")?
            .json()
            .context("parsing Drydock runs response")?;

        Ok(response.data)
    }

    #[allow(dead_code)]
    pub fn get_run(&self, run_id: i64) -> Result<DrydockRun> {
        self.client
            .get(self.endpoint(&format!("runs/{run_id}"))?)
            .send()
            .with_context(|| format!("requesting Drydock run {run_id}"))?
            .error_for_status()
            .with_context(|| format!("Drydock run {run_id} request failed"))?
            .json()
            .with_context(|| format!("parsing Drydock run {run_id}"))
    }

    pub fn update_run(&self, run_id: i64, update: &RunUpdate) -> Result<()> {
        self.client
            .patch(self.endpoint(&format!("runs/{run_id}"))?)
            .json(update)
            .send()
            .with_context(|| format!("updating Drydock run {run_id}"))?
            .error_for_status()
            .with_context(|| format!("Drydock run {run_id} update failed"))?;
        Ok(())
    }

    pub fn update_item_status(&self, item_id: i64, status: &str) -> Result<()> {
        self.client
            .patch(self.endpoint(&format!("items/{item_id}"))?)
            .json(&ItemUpdate {
                status: status.to_string(),
            })
            .send()
            .with_context(|| format!("updating Drydock item {item_id}"))?
            .error_for_status()
            .with_context(|| format!("Drydock item {item_id} update failed"))?;
        Ok(())
    }

    pub fn paused(&self) -> Result<bool> {
        let response: PauseFlag = self
            .client
            .get(self.endpoint("meta/paused")?)
            .send()
            .context("requesting Drydock pause flag")?
            .error_for_status()
            .context("Drydock pause flag request failed")?
            .json()
            .context("parsing Drydock pause flag")?;

        Ok(response.paused)
    }

    pub fn set_paused(&self, paused: bool) -> Result<()> {
        self.client
            .put(self.endpoint("meta/paused")?)
            .json(&PauseFlag { paused })
            .send()
            .context("updating Drydock pause flag")?
            .error_for_status()
            .context("Drydock pause flag update failed")?;
        Ok(())
    }

    fn endpoint(&self, path: &str) -> Result<Url> {
        self.base_url
            .join(path)
            .with_context(|| format!("joining Drydock endpoint {path}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{DrydockClient, DrydockDataResponse};
    use crate::models::DrydockRun;

    #[test]
    fn endpoint_joins_root_urls_without_double_or_missing_slashes() {
        let client = DrydockClient::new("http://localhost:3211").unwrap();

        assert_eq!(
            client.endpoint("runs").unwrap().as_str(),
            "http://localhost:3211/runs"
        );
        assert_eq!(
            client.endpoint("meta/paused").unwrap().as_str(),
            "http://localhost:3211/meta/paused"
        );
    }

    #[test]
    fn endpoint_preserves_configured_path_prefixes() {
        let client = DrydockClient::new("http://localhost:3211/api").unwrap();

        assert_eq!(
            client.endpoint("runs").unwrap().as_str(),
            "http://localhost:3211/api/runs"
        );
    }

    #[test]
    fn list_runs_response_deserializes_from_data_wrapper() {
        let response: DrydockDataResponse<Vec<DrydockRun>> = serde_json::from_str(
            r#"{
                "data": [
                    {
                        "id": 42,
                        "item_id": 7,
                        "item_title": "Example item",
                        "repo": "owner/repo",
                        "branch": "main",
                        "status": "running",
                        "ci_status": "passed",
                        "review_status": "approved",
                        "retry_count": 2,
                        "pr_url": "https://example.invalid/pr/42",
                        "session_id": "session-123",
                        "started_at": "2026-03-30T12:34:56Z"
                    }
                ],
                "pagination": {
                    "page": 1,
                    "per_page": 50,
                    "total": 1
                }
            }"#,
        )
        .unwrap();

        let run = &response.data[0];
        assert_eq!(run.item_id, 7);
        assert_eq!(run.item_title.as_deref(), Some("Example item"));
        assert_eq!(run.session_id.as_deref(), Some("session-123"));
        assert_eq!(run.review_status.as_deref(), Some("approved"));
        assert_eq!(run.retry_count, 2);
        assert_eq!(run.repo, "owner/repo");
        assert_eq!(run.branch, "main");
        assert_eq!(run.pr_url.as_deref(), Some("https://example.invalid/pr/42"));
        assert_eq!(run.ci_status.as_deref(), Some("passed"));
        assert!(run.started_at.is_some());
    }
}
