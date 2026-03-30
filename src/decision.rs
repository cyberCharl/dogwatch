use std::cmp::Ordering;

use chrono::{DateTime, Datelike, Local, Timelike, Weekday};

use crate::github::PullRequest;
use crate::models::{CiStatus, DrydockRun, ReviewStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Nudge,
    Notify,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Ci,
    Reviews,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub action: ActionKind,
    pub summary: &'static str,
    pub nudge_message: Option<String>,
    pub failure_kind: Option<FailureKind>,
    pub update_item_to_evaluating: bool,
    pub update_run_to_evaluating: bool,
}

pub fn decide(pr: &PullRequest, ci_status: CiStatus, review_status: ReviewStatus) -> Decision {
    match (ci_status, review_status) {
        (CiStatus::Failed, _) => Decision {
            action: ActionKind::Nudge,
            summary: "CI failed",
            nudge_message: Some(format!(
                "CI has failed on PR #{}. Please investigate the failures using the gh cli and fix them.",
                pr.number
            )),
            failure_kind: Some(FailureKind::Ci),
            update_item_to_evaluating: false,
            update_run_to_evaluating: false,
        },
        (CiStatus::Passed, ReviewStatus::ChangesRequested)
        | (CiStatus::NoChecks, ReviewStatus::ChangesRequested) => Decision {
            action: ActionKind::Nudge,
            summary: "address comments",
            nudge_message: Some(format!(
                "There are review comments on PR #{} that need to be addressed. Please check and address the comments using the gh cli.",
                pr.number
            )),
            failure_kind: Some(FailureKind::Reviews),
            update_item_to_evaluating: false,
            update_run_to_evaluating: false,
        },
        (CiStatus::Pending, _) => Decision {
            action: ActionKind::Skip,
            summary: "CI pending",
            nudge_message: None,
            failure_kind: None,
            update_item_to_evaluating: false,
            update_run_to_evaluating: false,
        },
        (
            CiStatus::Passed,
            ReviewStatus::Approved | ReviewStatus::Pending | ReviewStatus::NoReviews,
        )
        | (
            CiStatus::NoChecks,
            ReviewStatus::Approved | ReviewStatus::Pending | ReviewStatus::NoReviews,
        ) => Decision {
            action: ActionKind::Notify,
            summary: "ready for review",
            nudge_message: None,
            failure_kind: None,
            update_item_to_evaluating: true,
            update_run_to_evaluating: true,
        },
    }
}

pub fn sort_candidates(
    now: DateTime<Local>,
    left: (&DrydockRun, &PullRequest),
    right: (&DrydockRun, &PullRequest),
) -> Ordering {
    priority_tuple(now, left.0, left.1).cmp(&priority_tuple(now, right.0, right.1))
}

fn priority_tuple(now: DateTime<Local>, run: &DrydockRun, pr: &PullRequest) -> (u8, u8, i64) {
    let in_work_hours = is_sast_work_hours(now);
    let org_priority = if in_work_hours && repo_owner(&pr.repo) == Some("AI-Safety-SA") {
        0
    } else {
        1
    };
    let started_at = run.started_at.map(|value| value.timestamp()).unwrap_or(0);
    (org_priority, run.item_priority.rank(), -started_at)
}

pub fn is_sast_work_hours(now: DateTime<Local>) -> bool {
    let weekday = now.weekday();
    matches!(
        weekday,
        Weekday::Mon | Weekday::Tue | Weekday::Wed | Weekday::Thu | Weekday::Fri
    ) && (8..17).contains(&now.hour())
}

fn repo_owner(repo: &str) -> Option<&str> {
    repo.split('/').next()
}

#[cfg(test)]
mod tests {
    use chrono::{Local, TimeZone};

    use super::{ActionKind, decide, is_sast_work_hours, sort_candidates};
    use crate::github::PullRequest;
    use crate::models::{CiStatus, DrydockRun, ItemPriority, ReviewStatus};

    fn pr(repo: &str, number: u64) -> PullRequest {
        PullRequest {
            repo: repo.to_string(),
            number,
            title: "Example".to_string(),
            head_ref_name: "branch".to_string(),
            url: "https://example.invalid".to_string(),
        }
    }

    fn run(repo: &str, priority: ItemPriority) -> DrydockRun {
        DrydockRun {
            id: 1,
            item_id: Some(1),
            item_title: Some("Item".to_string()),
            repo: repo.to_string(),
            branch: "branch".to_string(),
            status: "running".to_string(),
            ci_status: None,
            review_status: None,
            retry_count: 0,
            pr_url: None,
            notes: None,
            session_id: None,
            started_at: Some(
                Local
                    .with_ymd_and_hms(2026, 3, 30, 10, 0, 0)
                    .unwrap()
                    .to_utc(),
            ),
            item_priority: priority,
        }
    }

    #[test]
    fn changes_requested_with_passed_ci_nudges() {
        let decision = decide(
            &pr("owner/repo", 12),
            CiStatus::Passed,
            ReviewStatus::ChangesRequested,
        );
        assert_eq!(decision.action, ActionKind::Nudge);
        assert_eq!(decision.summary, "address comments");
    }

    #[test]
    fn approved_without_checks_notifies() {
        let decision = decide(
            &pr("owner/repo", 12),
            CiStatus::NoChecks,
            ReviewStatus::Approved,
        );
        assert_eq!(decision.action, ActionKind::Notify);
        assert!(decision.update_item_to_evaluating);
    }

    #[test]
    fn sast_hours_prioritize_ai_safety_sa() {
        let now = Local.with_ymd_and_hms(2026, 3, 30, 9, 0, 0).unwrap();
        let mut entries = [
            (
                run("other/repo", ItemPriority::Critical),
                pr("other/repo", 1),
            ),
            (
                run("AI-Safety-SA/repo", ItemPriority::Low),
                pr("AI-Safety-SA/repo", 2),
            ),
        ];

        entries
            .sort_by(|left, right| sort_candidates(now, (&left.0, &left.1), (&right.0, &right.1)));
        assert_eq!(entries[0].1.repo, "AI-Safety-SA/repo");
    }

    #[test]
    fn sast_hours_detection_matches_weekday_window() {
        let monday = Local.with_ymd_and_hms(2026, 3, 30, 8, 30, 0).unwrap();
        let saturday = Local.with_ymd_and_hms(2026, 4, 4, 10, 0, 0).unwrap();
        assert!(is_sast_work_hours(monday));
        assert!(!is_sast_work_hours(saturday));
    }
}
