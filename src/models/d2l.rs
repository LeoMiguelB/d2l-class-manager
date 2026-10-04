use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WhoAmIResponse {
    pub identifier: String,
    pub first_name: String,
    pub last_name: String,
    pub unique_name: String,
    pub profile_badge_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BookmarkPagedResult<T> {
    pub paging_info: PagingInfo,
    #[serde(default = "Vec::new")]
    pub items: Vec<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PagingInfo {
    pub bookmark: Option<String>,
    pub has_more_items: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MyEnrollment {
    pub org_unit: OrgUnitInfo,
    pub access: AccessInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct OrgUnitInfo {
    pub id: i64,
    pub name: String,
    pub code: Option<String>,
    #[serde(rename = "Type")]
    pub unit_type: OrgTypeInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct OrgTypeInfo {
    pub id: i64,
    pub code: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AccessInfo {
    pub is_active: bool,
    pub can_access: bool,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RichText {
    pub text: Option<String>,
    pub html: Option<String>,
}

impl RichText {
    pub fn to_plain_markdown(&self) -> String {
        if let Some(ref html) = self.html {
            html2text::from_read(html.as_bytes(), 80).unwrap_or_else(|_| html.clone())
        } else if let Some(ref text) = self.text {
            text.clone()
        } else {
            String::new()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NewsItem {
    pub id: i64,
    pub title: String,
    pub body: RichText,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub is_published: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DropboxFolder {
    pub id: i64,
    pub name: String,
    pub custom_instructions: Option<RichText>,
    #[serde(default)]
    pub attachments: Vec<AttachmentInfo>,
    pub total_points: Option<f64>,
    pub due_date: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    #[serde(default)]
    pub is_hidden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AttachmentInfo {
    pub file_id: i64,
    pub file_name: String,
    #[serde(default)]
    pub size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Submission {
    pub id: i64,
    pub submission_date: String,
    #[serde(default)]
    pub files: Vec<SubmissionFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SubmissionFile {
    pub file_id: i64,
    pub file_name: String,
    #[serde(default)]
    pub size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContentToc {
    #[serde(default)]
    pub modules: Vec<ContentModule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContentModule {
    pub module_id: i64,
    pub title: String,
    #[serde(default)]
    pub modules: Vec<ContentModule>,
    #[serde(default)]
    pub topics: Vec<ContentTopic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContentTopic {
    pub topic_id: i64,
    pub identifier: String,
    pub type_identifier: String,
    pub title: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DiscussionForum {
    pub forum_id: i64,
    pub name: String,
    pub description: Option<RichText>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DiscussionTopic {
    pub topic_id: i64,
    pub name: String,
    pub description: Option<RichText>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DiscussionPost {
    pub post_id: i64,
    pub parent_post_id: Option<i64>,
    pub subject: String,
    pub message: RichText,
    pub date_posted: String,
    pub posting_user_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CalendarEvent {
    pub event_id: i64,
    pub title: String,
    pub description: Option<RichText>,
    pub start_date_time: String,
    pub end_date_time: String,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Quiz {
    pub quiz_id: i64,
    pub name: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub due_date: Option<String>,
    pub time_limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GradeItem {
    pub id: i64,
    pub name: String,
    pub points_numerator: Option<f64>,
    pub points_denominator: Option<f64>,
    pub weighted_numerator: Option<f64>,
    pub weighted_denominator: Option<f64>,
    pub comments: Option<RichText>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_richtext_html_to_markdown() {
        let rt = RichText {
            text: None,
            html: Some("<p>Welcome to <strong>CIS*4300</strong>! Check the <a href=\"https://example.com\">syllabus</a>.</p>".to_string()),
        };
        let md = rt.to_plain_markdown();
        assert!(md.contains("CIS*4300"));
        assert!(md.contains("syllabus"));
    }

    #[test]
    fn test_dropbox_folder_deserialization_defaults() {
        // Missing attachments, custom_instructions, total_points, etc.
        let json_data = r#"{
            "Id": 12345,
            "Name": "Milestone 1",
            "DueDate": "2026-10-09T23:59:00Z"
        }"#;

        let folder: DropboxFolder = serde_json::from_str(json_data).unwrap();
        assert_eq!(folder.id, 12345);
        assert_eq!(folder.name, "Milestone 1");
        assert_eq!(folder.attachments.len(), 0);
        assert!(!folder.is_hidden);
    }

    #[test]
    fn test_paged_result_deserialization_defaults() {
        let json_data = r#"{
            "PagingInfo": {
                "Bookmark": null,
                "HasMoreItems": false
            }
        }"#;

        let res: BookmarkPagedResult<String> = serde_json::from_str(json_data).unwrap();
        assert_eq!(res.items.len(), 0);
        assert!(!res.paging_info.has_more_items);
    }
}
