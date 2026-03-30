use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};

use crate::cli::LogLevelArg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

impl From<LogLevelArg> for LogLevel {
    fn from(value: LogLevelArg) -> Self {
        match value {
            LogLevelArg::Info => Self::Info,
            LogLevelArg::Warn => Self::Warn,
            LogLevelArg::Error => Self::Error,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub run_id: Option<i64>,
    pub repo: Option<String>,
    pub pr_number: Option<u64>,
    pub action: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct NewLogEntry<'a> {
    pub level: LogLevel,
    pub run_id: Option<i64>,
    pub repo: Option<&'a str>,
    pub pr_number: Option<u64>,
    pub action: &'a str,
    pub message: &'a str,
}

#[derive(Debug, Clone)]
pub struct StaleTracker {
    pub cycles: u32,
    pub notified: bool,
}

pub struct DbLogger {
    conn: Connection,
    max_rows: u32,
}

impl DbLogger {
    pub fn new(path: &Path, max_rows: u32) -> Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut logger = Self { conn, max_rows };
        logger.initialize()?;
        logger.rotate()?;
        Ok(logger)
    }

    pub fn record(&mut self, entry: NewLogEntry<'_>) -> Result<()> {
        let timestamp = Utc::now();
        self.conn
            .execute(
                "INSERT INTO logs (timestamp, level, run_id, repo, pr_number, action, message)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    timestamp.to_rfc3339(),
                    entry.level.as_str(),
                    entry.run_id,
                    entry.repo,
                    entry.pr_number.map(|value| value as i64),
                    entry.action,
                    entry.message,
                ],
            )
            .context("inserting log row")?;

        println!(
            "{} [{}] {} {}",
            timestamp.to_rfc3339(),
            entry.level.as_str().to_uppercase(),
            entry.action,
            entry.message
        );
        Ok(())
    }

    pub fn query(
        &self,
        last: usize,
        run_id: Option<i64>,
        level: Option<LogLevel>,
    ) -> Result<Vec<LogEntry>> {
        let mut statement = self.conn.prepare(
            "SELECT timestamp, level, run_id, repo, pr_number, action, message
             FROM logs
             WHERE (?1 IS NULL OR run_id = ?1)
               AND (?2 IS NULL OR level = ?2)
             ORDER BY id DESC
             LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![run_id, level.map(LogLevel::as_str), last as i64],
            |row| {
                let timestamp: String = row.get(0)?;
                let level: String = row.get(1)?;
                Ok(LogEntry {
                    timestamp: DateTime::parse_from_rfc3339(&timestamp).unwrap().to_utc(),
                    level: match level.as_str() {
                        "info" => LogLevel::Info,
                        "warn" => LogLevel::Warn,
                        _ => LogLevel::Error,
                    },
                    run_id: row.get(2)?,
                    repo: row.get(3)?,
                    pr_number: row.get::<_, Option<i64>>(4)?.map(|value| value as u64),
                    action: row.get(5)?,
                    message: row.get(6)?,
                })
            },
        )?;

        let mut entries = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("reading log rows")?;
        entries.reverse();
        Ok(entries)
    }

    pub fn bump_stale_cycle(&mut self, run_id: i64) -> Result<StaleTracker> {
        let existing: Option<(u32, Option<String>)> = self
            .conn
            .query_row(
                "SELECT cycles, notified_at FROM stale_runs WHERE run_id = ?1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .context("reading stale cycle row")?;

        let now = Utc::now().to_rfc3339();
        let tracker = match existing {
            Some((cycles, notified_at)) => {
                let next = cycles + 1;
                self.conn.execute(
                    "UPDATE stale_runs SET cycles = ?2, last_seen_at = ?3 WHERE run_id = ?1",
                    params![run_id, next, now],
                )?;
                StaleTracker {
                    cycles: next,
                    notified: notified_at.is_some(),
                }
            }
            None => {
                self.conn.execute(
                    "INSERT INTO stale_runs (run_id, cycles, last_seen_at) VALUES (?1, 1, ?2)",
                    params![run_id, now],
                )?;
                StaleTracker {
                    cycles: 1,
                    notified: false,
                }
            }
        };

        Ok(tracker)
    }

    pub fn mark_long_running_notified(&mut self, run_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE stale_runs SET notified_at = ?2 WHERE run_id = ?1",
            params![run_id, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn clear_stale_cycle(&mut self, run_id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM stale_runs WHERE run_id = ?1", params![run_id])?;
        Ok(())
    }

    fn initialize(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                level TEXT NOT NULL,
                run_id INTEGER,
                repo TEXT,
                pr_number INTEGER,
                action TEXT NOT NULL,
                message TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS stale_runs (
                run_id INTEGER PRIMARY KEY,
                cycles INTEGER NOT NULL,
                last_seen_at TEXT NOT NULL,
                notified_at TEXT
            );
            ",
        )?;
        Ok(())
    }

    fn rotate(&mut self) -> Result<()> {
        let total_rows: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))
            .context("counting log rows")?;

        if total_rows <= i64::from(self.max_rows) {
            return Ok(());
        }

        self.conn.execute(
            "DELETE FROM logs
             WHERE id IN (
                SELECT id FROM logs
                ORDER BY id ASC
                LIMIT ?1
             )",
            params![total_rows - 4000],
        )?;
        Ok(())
    }
}
