use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct DrydockRun {
    pub id: i64,
    #[serde(default)]
    pub item_id: Option<i64>,
    #[serde(default)]
    pub item_title: Option<String>,
    pub repo: String,
    pub branch: String,
    pub status: String,
    #[serde(default)]
    pub ci_status: Option<String>,
    #[serde(default)]
    pub review_status: Option<String>,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default)]
    pub pr_url: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub item_priority: ItemPriority,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ItemPriority {
    Critical,
    High,
    Medium,
    Low,
    #[default]
    None,
}

impl ItemPriority {
    pub fn rank(self) -> u8 {
        match self {
            Self::Critical => 0,
            Self::High => 1,
            Self::Medium => 2,
            Self::Low => 3,
            Self::None => 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RunUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ci_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ItemUpdate {
    pub status: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PauseFlagResponse {
    #[serde(default)]
    pub paused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiStatus {
    Passed,
    Failed,
    Pending,
    NoChecks,
}

impl CiStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Pending => "pending",
            Self::NoChecks => "no-checks",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewStatus {
    Approved,
    ChangesRequested,
    Pending,
    NoReviews,
}

impl ReviewStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::ChangesRequested => "changes_requested",
            Self::Pending => "pending",
            Self::NoReviews => "no_reviews",
        }
    }
}
