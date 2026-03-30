use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use chrono::Local;
use clap::Parser;

use crate::agent::{self, SessionState};
use crate::cli::{Cli, Command};
use crate::config::{LoadedConfig, config_display_path, local_pause_enabled, set_local_pause};
use crate::decision::{ActionKind, FailureKind, decide, sort_candidates};
use crate::drydock::DrydockClient;
use crate::github::{GitHubClient, PullRequest};
use crate::logging::{DbLogger, LogLevel, NewLogEntry};
use crate::models::{CiStatus, DrydockRun, RunUpdate};
use crate::notifications::Notifier;

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let loaded = LoadedConfig::load()?;
    let mut app = App::new(cli, loaded)?;

    match app.cli.command.clone().unwrap_or(Command::Check) {
        Command::Check => app.check(),
        Command::Status => app.status(),
        Command::Logs(args) => app.logs(args.last, args.run_id, args.level.map(Into::into)),
        Command::Pause => app.set_pause(true),
        Command::Unpause => app.set_pause(false),
    }
}

struct App {
    cli: Cli,
    config: LoadedConfig,
    drydock: DrydockClient,
    github: GitHubClient,
    notifier: Notifier,
    logger: DbLogger,
}

#[derive(Clone)]
struct MatchedRun {
    pr: PullRequest,
    run: DrydockRun,
    ci_status: CiStatus,
    review_status: crate::models::ReviewStatus,
}

impl App {
    fn new(cli: Cli, config: LoadedConfig) -> Result<Self> {
        let logger = DbLogger::new(
            &config.paths.database_file,
            config.config.limits.log_max_rows,
        )?;
        let drydock = DrydockClient::new(&config.config.drydock.api_url)?;
        let github = GitHubClient::new(cli.verbose);
        let notifier = Notifier::new(
            config.config.notifications.telegram_target.clone(),
            cli.verbose,
        );

        Ok(Self {
            cli,
            config,
            drydock,
            github,
            notifier,
            logger,
        })
    }

    fn check(&mut self) -> Result<()> {
        if local_pause_enabled(&self.config.paths) {
            self.log(
                LogLevel::Info,
                None,
                None,
                None,
                "check",
                "local pause is enabled; skipping cycle",
            )?;
            return Ok(());
        }

        if self.drydock.paused()? {
            self.log(
                LogLevel::Info,
                None,
                None,
                None,
                "check",
                "Drydock pause flag is enabled; skipping cycle",
            )?;
            return Ok(());
        }

        let running_runs = self
            .drydock
            .list_runs(Some("running"), None, None)
            .context("listing running Drydock runs")?;
        let pull_requests = self
            .github
            .list_open_pull_requests()
            .context("listing open pull requests")?;
        let open_pr_keys = pull_requests
            .iter()
            .map(|pr| run_key(&pr.repo, &pr.head_ref_name))
            .collect::<HashSet<_>>();
        let run_index = latest_run_by_key(&running_runs);
        let mut matched = Vec::new();
        let mut matched_run_ids = HashSet::new();

        for pr in pull_requests {
            if let Some(run) = run_index
                .get(&run_key(&pr.repo, &pr.head_ref_name))
                .cloned()
            {
                matched_run_ids.insert(run.id);
                self.logger.clear_stale_cycle(run.id)?;
                let ci_status = self.github.pull_request_checks(&pr)?;
                let review_status = self.github.pull_request_review_status(&pr)?;
                matched.push(MatchedRun {
                    pr,
                    run,
                    ci_status,
                    review_status,
                });
            } else {
                self.log(
                    LogLevel::Info,
                    None,
                    Some(&pr.repo),
                    Some(pr.number),
                    "skip",
                    "orphan PR detected; no matching Drydock run",
                )?;
            }
        }

        let now = Local::now();
        matched.sort_by(|left, right| {
            sort_candidates(now, (&left.run, &left.pr), (&right.run, &right.pr))
        });

        for entry in matched {
            self.process_matched(entry)?;
        }

        self.process_stale_runs(&running_runs, &open_pr_keys, &matched_run_ids)
    }

    fn process_matched(&mut self, entry: MatchedRun) -> Result<()> {
        let decision = decide(&entry.pr, entry.ci_status, entry.review_status);
        let mut sync_update = RunUpdate {
            ci_status: Some(entry.ci_status.as_str().to_string()),
            review_status: Some(entry.review_status.as_str().to_string()),
            retry_count: None,
            pr_url: Some(entry.pr.url.clone()),
            notes: None,
            status: None,
        };
        self.apply_run_update(
            &entry.run,
            &sync_update,
            "check",
            "synchronized run metadata",
        )?;

        match decision.action {
            ActionKind::Skip => {
                self.log(
                    LogLevel::Info,
                    Some(entry.run.id),
                    Some(&entry.pr.repo),
                    Some(entry.pr.number),
                    "skip",
                    decision.summary,
                )?;
            }
            ActionKind::Notify => {
                let notification = self
                    .notifier
                    .ready_for_review(&entry.pr, self.cli.dry_run)?;
                self.log(
                    LogLevel::Info,
                    Some(entry.run.id),
                    Some(&entry.pr.repo),
                    Some(entry.pr.number),
                    "notify",
                    &notification,
                )?;

                if decision.update_item_to_evaluating {
                    if let Some(item_id) = entry.run.item_id {
                        self.apply_item_status(item_id, "evaluating")?;
                    } else {
                        self.log(
                            LogLevel::Warn,
                            Some(entry.run.id),
                            Some(&entry.pr.repo),
                            Some(entry.pr.number),
                            "notify",
                            "run has no item_id; cannot move item to evaluating",
                        )?;
                    }
                }

                if decision.update_run_to_evaluating {
                    sync_update.status = Some("evaluating".to_string());
                    self.apply_run_update(
                        &entry.run,
                        &sync_update,
                        "notify",
                        "marked run evaluating",
                    )?;
                }
            }
            ActionKind::Nudge => {
                let failure_kind = decision
                    .failure_kind
                    .context("nudge decision missing failure kind")?;
                self.handle_nudge(entry, failure_kind, decision.summary)?;
            }
        }

        Ok(())
    }

    fn handle_nudge(
        &mut self,
        entry: MatchedRun,
        failure_kind: FailureKind,
        summary: &str,
    ) -> Result<()> {
        if entry.run.retry_count >= self.config.config.limits.max_retries {
            let notes = append_note(
                entry.run.notes.as_deref(),
                &format!(
                    "Retry limit reached after {} attempts ({summary}).",
                    entry.run.retry_count
                ),
            );
            self.apply_run_update(
                &entry.run,
                &RunUpdate {
                    notes: Some(notes),
                    ..RunUpdate::default()
                },
                "escalate",
                "retry limit reached",
            )?;
            let notification = self.notifier.retry_limit_reached(
                &entry.pr,
                entry.run.retry_count,
                failure_kind,
                self.cli.dry_run,
            )?;
            self.log(
                LogLevel::Warn,
                Some(entry.run.id),
                Some(&entry.pr.repo),
                Some(entry.pr.number),
                "escalate",
                &notification,
            )?;
            return Ok(());
        }

        let session_id = match entry.run.session_id.as_deref() {
            Some(value) => value,
            None => {
                self.fail_for_missing_session(&entry.pr, &entry.run, "run is missing session_id")?;
                return Ok(());
            }
        };

        match agent::session_state(&self.config.config.paths.acpx_sessions, session_id) {
            Ok(SessionState::Alive { session_name }) => {
                let message = decide(&entry.pr, entry.ci_status, entry.review_status)
                    .nudge_message
                    .context("missing nudge message")?;
                agent::send_nudge(&session_name, &message, self.cli.dry_run, self.cli.verbose)?;
                let next_retry_count = entry.run.retry_count + 1;
                let notes = append_note(
                    entry.run.notes.as_deref(),
                    &format!("Sent nudge #{next_retry_count} for {summary}."),
                );
                self.apply_run_update(
                    &entry.run,
                    &RunUpdate {
                        retry_count: Some(next_retry_count),
                        notes: Some(notes),
                        ..RunUpdate::default()
                    },
                    "nudge",
                    "nudged coding agent",
                )?;
                self.log(
                    LogLevel::Info,
                    Some(entry.run.id),
                    Some(&entry.pr.repo),
                    Some(entry.pr.number),
                    "nudge",
                    summary,
                )?;
            }
            Ok(SessionState::Closed) => {
                self.fail_for_missing_session(&entry.pr, &entry.run, "agent session is closed")?;
            }
            Err(error) => {
                self.fail_for_missing_session(
                    &entry.pr,
                    &entry.run,
                    &format!("failed to inspect session: {error}"),
                )?;
            }
        }

        Ok(())
    }

    fn fail_for_missing_session(
        &mut self,
        pr: &PullRequest,
        run: &DrydockRun,
        detail: &str,
    ) -> Result<()> {
        let notes = append_note(
            run.notes.as_deref(),
            &format!("Unable to nudge agent: {detail}."),
        );
        self.apply_run_update(
            run,
            &RunUpdate {
                status: Some("failed".to_string()),
                notes: Some(notes),
                ..RunUpdate::default()
            },
            "escalate",
            "marked run failed because the agent session is unavailable",
        )?;
        let notification = self.notifier.session_unavailable(pr, self.cli.dry_run)?;
        self.log(
            LogLevel::Error,
            Some(run.id),
            Some(&pr.repo),
            Some(pr.number),
            "escalate",
            &notification,
        )
    }

    fn process_stale_runs(
        &mut self,
        running_runs: &[DrydockRun],
        open_pr_keys: &HashSet<(String, String)>,
        matched_run_ids: &HashSet<i64>,
    ) -> Result<()> {
        for run in running_runs {
            if matched_run_ids.contains(&run.id)
                || open_pr_keys.contains(&run_key(&run.repo, &run.branch))
            {
                self.logger.clear_stale_cycle(run.id)?;
                continue;
            }

            let Some(session_id) = run.session_id.as_deref() else {
                self.log(
                    LogLevel::Warn,
                    Some(run.id),
                    Some(&run.repo),
                    None,
                    "check",
                    "running run has no session_id; cannot determine staleness",
                )?;
                continue;
            };

            match agent::session_state(&self.config.config.paths.acpx_sessions, session_id) {
                Ok(SessionState::Closed) => {
                    let notes =
                        append_note(run.notes.as_deref(), "Session closed and no open PR found.");
                    self.apply_run_update(
                        run,
                        &RunUpdate {
                            status: Some("failed".to_string()),
                            notes: Some(notes),
                            ..RunUpdate::default()
                        },
                        "escalate",
                        "marked stale run failed",
                    )?;
                    let notification = self.notifier.stale_run(run, self.cli.dry_run)?;
                    self.log(
                        LogLevel::Warn,
                        Some(run.id),
                        Some(&run.repo),
                        None,
                        "escalate",
                        &notification,
                    )?;
                    self.logger.clear_stale_cycle(run.id)?;
                }
                Ok(SessionState::Alive { .. }) => {
                    let tracker = self.logger.bump_stale_cycle(run.id)?;
                    self.log(
                        LogLevel::Warn,
                        Some(run.id),
                        Some(&run.repo),
                        None,
                        "check",
                        "running run has no open PR but session is still active",
                    )?;
                    if tracker.cycles >= self.config.config.limits.stale_cycles && !tracker.notified
                    {
                        let notification = self.notifier.long_running(run, self.cli.dry_run)?;
                        self.log(
                            LogLevel::Info,
                            Some(run.id),
                            Some(&run.repo),
                            None,
                            "notify",
                            &notification,
                        )?;
                        self.logger.mark_long_running_notified(run.id)?;
                    }
                }
                Err(error) => {
                    self.log(
                        LogLevel::Warn,
                        Some(run.id),
                        Some(&run.repo),
                        None,
                        "check",
                        &format!("failed to inspect session {session_id}: {error}"),
                    )?;
                }
            }
        }
        Ok(())
    }

    fn status(&mut self) -> Result<()> {
        let local_pause = local_pause_enabled(&self.config.paths);
        let remote_pause = self.drydock.paused().unwrap_or(false);

        println!(
            "config: {}",
            config_display_path(&self.config.paths.config_file)
        );
        println!("data: {}", config_display_path(&self.config.paths.data_dir));
        println!(
            "database: {}",
            config_display_path(&self.config.paths.database_file)
        );
        println!(
            "acpx_sessions: {}",
            config_display_path(&self.config.config.paths.acpx_sessions)
        );
        println!("local_pause: {}", local_pause);
        println!("drydock_pause: {}", remote_pause);
        println!("dry_run: {}", self.cli.dry_run);
        println!("once: {}", self.cli.once);
        Ok(())
    }

    fn logs(&mut self, last: usize, run_id: Option<i64>, level: Option<LogLevel>) -> Result<()> {
        for entry in self.logger.query(last, run_id, level)? {
            println!(
                "{} [{}] run={:?} repo={:?} pr={:?} {} {}",
                entry.timestamp.to_rfc3339(),
                entry.level.as_str(),
                entry.run_id,
                entry.repo,
                entry.pr_number,
                entry.action,
                entry.message
            );
        }
        Ok(())
    }

    fn set_pause(&mut self, paused: bool) -> Result<()> {
        if self.cli.dry_run {
            println!(
                "dry-run: would {} {}",
                if paused { "create" } else { "remove" },
                self.config.paths.pause_file.display()
            );
        } else {
            set_local_pause(&self.config.paths, paused)?;
        }

        self.log(
            LogLevel::Info,
            None,
            None,
            None,
            "check",
            if paused {
                "local pause enabled"
            } else {
                "local pause disabled"
            },
        )
    }

    fn apply_run_update(
        &mut self,
        run: &DrydockRun,
        update: &RunUpdate,
        action: &str,
        message: &str,
    ) -> Result<()> {
        if self.cli.dry_run {
            self.log(
                LogLevel::Info,
                Some(run.id),
                Some(&run.repo),
                None,
                action,
                message,
            )?;
            return Ok(());
        }

        self.drydock.update_run(run.id, update)?;
        self.log(
            LogLevel::Info,
            Some(run.id),
            Some(&run.repo),
            None,
            action,
            message,
        )
    }

    fn apply_item_status(&mut self, item_id: i64, status: &str) -> Result<()> {
        if self.cli.dry_run {
            self.log(
                LogLevel::Info,
                None,
                None,
                None,
                "notify",
                &format!("would update Drydock item {item_id} to {status}"),
            )?;
            return Ok(());
        }

        self.drydock.update_item_status(item_id, status)?;
        self.log(
            LogLevel::Info,
            None,
            None,
            None,
            "notify",
            &format!("updated Drydock item {item_id} to {status}"),
        )
    }

    fn log(
        &mut self,
        level: LogLevel,
        run_id: Option<i64>,
        repo: Option<&str>,
        pr_number: Option<u64>,
        action: &str,
        message: &str,
    ) -> Result<()> {
        self.logger.record(NewLogEntry {
            level,
            run_id,
            repo,
            pr_number,
            action,
            message,
        })
    }
}

fn latest_run_by_key(runs: &[DrydockRun]) -> HashMap<(String, String), DrydockRun> {
    let mut latest: HashMap<(String, String), DrydockRun> = HashMap::new();
    for run in runs {
        let key = run_key(&run.repo, &run.branch);
        match latest.get(&key) {
            Some(existing) if run.started_at <= existing.started_at => {}
            _ => {
                latest.insert(key, run.clone());
            }
        }
    }
    latest
}

fn run_key(repo: &str, branch: &str) -> (String, String) {
    (repo.to_string(), branch.to_string())
}

fn append_note(existing: Option<&str>, addition: &str) -> String {
    match existing {
        Some(current) if !current.trim().is_empty() => format!("{current}\n{addition}"),
        _ => addition.to_string(),
    }
}
