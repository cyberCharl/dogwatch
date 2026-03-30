use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};

fn null_to_empty<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct DrydockRun {
    pub id: i64,
    pub item_id: i64,
    #[serde(default)]
    pub item_title: Option<String>,
    #[serde(default, deserialize_with = "null_to_empty")]
    pub repo: String,
    #[serde(default, deserialize_with = "null_to_empty")]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PauseFlag {
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
