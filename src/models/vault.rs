use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourseVault {
    pub course_code: String,
    pub course_name: String,
    #[serde(default)]
    pub instructor: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub links: Option<HashMap<String, String>>,
    #[serde(default)]
    pub policies: Option<HashMap<String, String>>,
    #[serde(default)]
    pub items: Vec<DeadlineItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemType {
    Exam,
    Assignment,
    ProjectMilestone,
    Critique,
    Presentation,
    Admin,
    Participation,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Todo,
    InProgress,
    Completed,
    Dropped,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadlineItem {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub item_type: ItemType,
    pub due_date: DateTime<FixedOffset>,
    #[serde(default)]
    pub weight: f64,
    #[serde(default = "default_status")]
    pub status: ItemStatus,
    #[serde(default)]
    pub submission_platform: Option<String>,
    #[serde(default)]
    pub lead_time_days: Option<i64>,
    #[serde(default)]
    pub notes: Option<String>,
}

fn default_status() -> ItemStatus {
    ItemStatus::Todo
}
