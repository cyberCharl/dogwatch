use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};

use crate::decision::FailureKind;
use crate::github::PullRequest;
use crate::models::DrydockRun;

#[derive(Debug, Clone)]
pub struct Notifier {
    target: Option<String>,
    verbose: bool,
}

impl Notifier {
    pub fn new(target: Option<String>, verbose: bool) -> Self {
        Self { target, verbose }
    }

    pub fn ready_for_review(&self, pr: &PullRequest, dry_run: bool) -> Result<String> {
        let message = format!(
            "✅ PR ready for review: {} #{} — {}\n   {}",
            pr.repo, pr.number, pr.title, pr.url
        );
        self.send(&message, dry_run)?;
        Ok(message)
    }

    pub fn retry_limit_reached(
        &self,
        pr: &PullRequest,
        attempts: u32,
        failure: FailureKind,
        dry_run: bool,
    ) -> Result<String> {
        let failure = match failure {
            FailureKind::Ci => "ci",
            FailureKind::Reviews => "reviews",
        };
        let message = format!(
            "🔴 Retry limit reached: {} #{} — {}\n   {} ({} attempts, last failure: {})",
            pr.repo, pr.number, pr.title, pr.url, attempts, failure
        );
        self.send(&message, dry_run)?;
        Ok(message)
    }

    pub fn stale_run(&self, run: &DrydockRun, dry_run: bool) -> Result<String> {
        let message = format!(
            "⚠️ Stale run: \"{}\" (run #{}) — session dead, no PR found",
            item_title(run),
            run.id
        );
        self.send(&message, dry_run)?;
        Ok(message)
    }

    pub fn long_running(&self, run: &DrydockRun, dry_run: bool) -> Result<String> {
        let message = format!(
            "⏳ Long-running: \"{}\" (run #{}) — session active, no PR after 45min",
            item_title(run),
            run.id
        );
        self.send(&message, dry_run)?;
        Ok(message)
    }

    pub fn session_unavailable(&self, pr: &PullRequest, dry_run: bool) -> Result<String> {
        let message = format!(
            "🔴 Agent session unavailable: {} #{} — {}\n   {}",
            pr.repo, pr.number, pr.title, pr.url
        );
        self.send(&message, dry_run)?;
        Ok(message)
    }

    fn send(&self, message: &str, dry_run: bool) -> Result<()> {
        if dry_run {
            return Ok(());
        }

        let target = self
            .target
            .as_deref()
            .context("notifications.telegram_target is not configured")?;

        if self.verbose {
            eprintln!("> openclaw message send --channel telegram --target {target} {message}");
        }

        let output = Command::new("openclaw")
            .args([
                "message",
                "send",
                "--channel",
                "telegram",
                "--target",
                target,
                message,
            ])
            .stdin(Stdio::null())
            .output()
            .context("spawning `openclaw`")?;

        if !output.status.success() {
            return Err(anyhow!(
                "`openclaw` failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        Ok(())
    }
}

fn item_title(run: &DrydockRun) -> &str {
    run.item_title.as_deref().unwrap_or("unknown item")
}
