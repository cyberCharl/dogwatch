use anyhow::{Context, Result};
use reqwest::Url;
use reqwest::blocking::Client;

use crate::models::{DrydockRun, ItemUpdate, PauseFlagResponse, RunUpdate};

#[derive(Debug, Clone)]
pub struct DrydockClient {
    client: Client,
    base_url: Url,
}

impl DrydockClient {
    pub fn new(api_url: &str) -> Result<Self> {
        let base_url =
            Url::parse(api_url).with_context(|| format!("invalid Drydock URL {api_url}"))?;
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

        self.client
            .get(url)
            .send()
            .context("requesting Drydock runs")?
            .error_for_status()
            .context("Drydock runs request failed")?
            .json()
            .context("parsing Drydock runs response")
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
        let response: PauseFlagResponse = self
            .client
            .get(self.endpoint("pause")?)
            .send()
            .context("requesting Drydock pause flag")?
            .error_for_status()
            .context("Drydock pause flag request failed")?
            .json()
            .context("parsing Drydock pause flag")?;

        Ok(response.paused)
    }

    fn endpoint(&self, path: &str) -> Result<Url> {
        self.base_url
            .join(path)
            .with_context(|| format!("joining Drydock endpoint {path}"))
    }
}
