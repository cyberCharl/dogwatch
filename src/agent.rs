use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub enum SessionState {
    Alive { session_name: String },
    Closed,
}

pub fn session_state(sessions_dir: &Path, session_id: &str) -> Result<SessionState> {
    let path = sessions_dir.join(format!("{session_id}.json"));
    let raw = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let session: SessionFile =
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;

    if session.closed.unwrap_or(false) {
        Ok(SessionState::Closed)
    } else {
        Ok(SessionState::Alive {
            session_name: session.name.unwrap_or_else(|| session_id.to_string()),
        })
    }
}

pub fn send_nudge(session_name: &str, message: &str, dry_run: bool, verbose: bool) -> Result<()> {
    if dry_run {
        return Ok(());
    }

    if verbose {
        eprintln!("> acpx codex prompt --session {session_name} {message}");
    }

    let output = Command::new("acpx")
        .args(["codex", "prompt", "--session", session_name, message])
        .stdin(Stdio::null())
        .output()
        .context("spawning `acpx`")?;

    if !output.status.success() {
        return Err(anyhow!(
            "`acpx` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct SessionFile {
    #[serde(default)]
    closed: Option<bool>,
    #[serde(default)]
    name: Option<String>,
}
