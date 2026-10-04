use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationReport {
    pub course_code: String,
    pub org_unit_id: i64,
    pub checked_at: DateTime<Utc>,
    pub discrepancies: Vec<Discrepancy>,
    pub announcements_for_ai: Vec<AnnouncementSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "details")]
pub enum Discrepancy {
    DueDateShift {
        item_id: String,
        item_title: String,
        vault_due_date: DateTime<FixedOffset>,
        lms_due_date: DateTime<FixedOffset>,
    },
    SubmittedAutoCompleted {
        item_id: String,
        item_title: String,
        submission_time: String,
        file_count: usize,
    },
    MissingInVault {
        lms_folder_id: i64,
        lms_title: String,
        lms_due_date: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnouncementSummary {
    pub id: i64,
    pub title: String,
    pub posted_date: Option<String>,
    pub markdown_body: String,
}
