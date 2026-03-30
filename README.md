# Dogwatch

Dogwatch is a Rust CLI that monitors open GitHub pull requests, correlates them to Drydock work items, checks CI and review state, nudges coding agents when work stalls, and notifies Telegram when PRs are ready for human review.

## Features

- `check` scans open PRs, matches them to Drydock runs, syncs CI/review metadata, nudges agents on failures, and sends review notifications.
- `status` prints the resolved config and pause state.
- `logs` queries the local SQLite log database.
- `pause` and `unpause` toggle the global pause flag in Drydock (via `PUT /meta/paused`).
- Drydock REST integration for runs, items, and the global pause flag.
- `gh` CLI integration for PR discovery, CI checks, and reviews.
- `acpx` integration for agent nudges.
- `openclaw` integration for Telegram notifications.

## Configuration

Dogwatch reads configuration from:

- `$XDG_CONFIG_HOME/dogwatch/config.toml`
- Fallback: `~/.config/dogwatch/config.toml`

Runtime data is stored in:

- `$XDG_DATA_HOME/dogwatch/`
- Fallback: `~/.local/share/dogwatch/`

Example configuration:

```toml
[drydock]
api_url = "http://localhost:3211"

[notifications]
telegram_target = "-1003725170652:3443"  # Telegram chat:topic format

[limits]
max_retries = 6
stale_cycles = 3
log_max_rows = 5000

[paths]
acpx_sessions = "/home/clawdysseus/.acpx/sessions"
```

## Usage

```bash
dogwatch --help
dogwatch check --dry-run
dogwatch status
dogwatch logs --last 100 --level warn
dogwatch pause
dogwatch unpause
```

`--dry-run`, `--verbose`, and `--once` are global flags and can be passed before any subcommand.

## Expected Integrations

- `gh` must be installed and authenticated.
- `acpx` must be available for nudge delivery.
- `openclaw` must be available for Telegram notifications.
- Drydock API endpoints used:
  - `GET /runs?status=...&repo=...&branch=...` — flat runs listing with filters
  - `PATCH /runs/:id` — update run status, CI/review metadata
  - `PATCH /items/:id` — update item status (building → evaluating)
  - `GET /meta/paused` — global pause flag
  - `PUT /meta/paused` — set global pause flag

## Logging

Dogwatch stores logs in `dogwatch.db` under the Dogwatch XDG data directory. Each run rotates old rows when the database grows beyond `limits.log_max_rows`.
