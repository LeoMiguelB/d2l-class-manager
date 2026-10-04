use crate::client::D2LClient;
use crate::models::reconcile::*;
use crate::models::vault::{CourseVault, DeadlineItem, ItemStatus};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::HashSet;

pub fn matches_item_title(folder_name: &str, item: &DeadlineItem) -> bool {
    let fn_norm = folder_name.trim().to_lowercase();
    let it_norm = item.title.trim().to_lowercase();
    let id_norm = item.id.trim().to_lowercase();

    // 1. Direct exact match
    if fn_norm == it_norm {
        return true;
    }

    // 2. Category clash check (e.g. assignment vs milestone vs quiz vs exam)
    let categories = ["assignment", "milestone", "lab", "project", "quiz", "exam", "midterm"];
    let fn_cat = categories.iter().find(|&&c| fn_norm.contains(c));
    let it_cat = categories
        .iter()
        .find(|&&c| it_norm.contains(c) || id_norm.contains(c));

    if let (Some(c1), Some(c2)) = (fn_cat, it_cat) {
        if c1 != c2 {
            return false;
        }
    }

    // 3. Digits check: if both contain digits, digits MUST match!
    let fn_digits: String = fn_norm.chars().filter(|c| c.is_ascii_digit()).collect();
    let it_digits: String = it_norm.chars().filter(|c| c.is_ascii_digit()).collect();
    if !fn_digits.is_empty() && !it_digits.is_empty() && fn_digits != it_digits {
        return false;
    }

    // 4. Substring match
    if it_norm.contains(&fn_norm) || fn_norm.contains(&it_norm) {
        return true;
    }

    // 5. Tokenized word overlap (ignoring punctuation and non-category stop words)
    let stop_words: HashSet<&str> = [
        "due", "the", "for", "in", "to", "and", "dropbox", "submission",
        "deliverable", "folder", "drop", "box"
    ]
    .into_iter()
    .collect();

    let fn_words: HashSet<String> = fn_norm
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !stop_words.contains(w))
        .map(|w| w.to_string())
        .collect();

    let it_words: HashSet<String> = it_norm
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !stop_words.contains(w))
        .map(|w| w.to_string())
        .collect();

    let intersection_count = fn_words.intersection(&it_words).count();
    if intersection_count >= 2
        || (intersection_count == 1 && (fn_words.len() == 1 || it_words.len() == 1))
    {
        return true;
    }

    // 6. ID match (e.g., id "cis4300-m1" matches folder "M1" or "Milestone 1")
    let id_suffix = id_norm.rsplit('-').next().unwrap_or("");
    if !id_suffix.is_empty() && (fn_norm.contains(id_suffix) || fn_words.contains(id_suffix)) {
        return true;
    }

    false
}

pub async fn build_reconciliation_report(
    client: &D2LClient,
    course_code: &str,
    org_unit_id: i64,
    vault: &CourseVault,
) -> Result<ReconciliationReport> {
    let folders = client.dropbox_folders(org_unit_id).await.unwrap_or_default();
    let news = client.news(org_unit_id, None).await.unwrap_or_default();

    let mut discrepancies = Vec::new();

    // 1. Compare Dropboxes against Vault items
    for folder in &folders {
        // Find matching vault item by robust match
        let matched_item = vault.items.iter().find(|i| matches_item_title(&folder.name, i));

        if let Some(item) = matched_item {
            // Check DueDate drift
            if let Some(ref lms_due_str) = folder.due_date {
                if let Ok(lms_due) = DateTime::parse_from_rfc3339(lms_due_str) {
                    let diff_mins = (lms_due.signed_duration_since(item.due_date)).num_minutes().abs();
                    if diff_mins > 60 {
                        discrepancies.push(Discrepancy::DueDateShift {
                            item_id: item.id.clone(),
                            item_title: item.title.clone(),
                            vault_due_date: item.due_date,
                            lms_due_date: lms_due,
                        });
                    }
                }
            }

            // Check if submitted on LMS but not marked done in vault
            if item.status != ItemStatus::Completed && item.status != ItemStatus::Dropped {
                if let Ok(subs) = client.submissions(org_unit_id, folder.id).await {
                    if !subs.is_empty() {
                        discrepancies.push(Discrepancy::SubmittedAutoCompleted {
                            item_id: item.id.clone(),
                            item_title: item.title.clone(),
                            submission_time: subs[0].submission_date.clone(),
                            file_count: subs[0].files.len(),
                        });
                    }
                }
            }
        } else {
            // Folder on LMS does not exist in vault
            discrepancies.push(Discrepancy::MissingInVault {
                lms_folder_id: folder.id,
                lms_title: folder.name.clone(),
                lms_due_date: folder.due_date.clone(),
            });
        }
    }

    // 2. Prepare announcement summaries for AI ingestion
    let mut announcements_for_ai = Vec::new();
    for n in news {
        announcements_for_ai.push(AnnouncementSummary {
            id: n.id,
            title: n.title,
            posted_date: n.start_date,
            markdown_body: n.body.to_plain_markdown(),
        });
    }

    Ok(ReconciliationReport {
        course_code: course_code.to_string(),
        org_unit_id,
        checked_at: Utc::now(),
        discrepancies,
        announcements_for_ai,
    })
}

pub fn apply_reconciliation_updates(vault: &mut CourseVault, report: &ReconciliationReport) -> usize {
    let mut count = 0;
    for disc in &report.discrepancies {
        match disc {
            Discrepancy::DueDateShift { item_id, lms_due_date, .. } => {
                if let Some(item) = vault.items.iter_mut().find(|i| &i.id == item_id) {
                    item.due_date = lms_due_date.with_timezone(&item.due_date.timezone());
                    count += 1;
                }
            }
            Discrepancy::SubmittedAutoCompleted { item_id, .. } => {
                if let Some(item) = vault.items.iter_mut().find(|i| &i.id == item_id) {
                    item.status = ItemStatus::Completed;
                    count += 1;
                }
            }
            Discrepancy::MissingInVault { .. } => {
                // Kept for user review or manual addition
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::vault::ItemType;
    use chrono::FixedOffset;

    #[test]
    fn test_matches_item_title() {
        let item = DeadlineItem {
            id: "cis4300-m1".to_string(),
            title: "Milestone 1 Due: Needfinding".to_string(),
            item_type: ItemType::ProjectMilestone,
            due_date: DateTime::parse_from_rfc3339("2026-10-09T23:59:00-04:00").unwrap(),
            weight: 10.0,
            status: ItemStatus::Todo,
            submission_platform: Some("CourseLink Dropbox".to_string()),
            lead_time_days: Some(12),
            notes: None,
        };

        // Exact & substring
        assert!(matches_item_title("Milestone 1 Due: Needfinding", &item));
        assert!(matches_item_title("Milestone 1", &item));

        // Punctuation and slight differences
        assert!(matches_item_title("Milestone 1 - Needfinding", &item));
        assert!(matches_item_title("Milestone 1: Needfinding (Drop Box)", &item));

        // Different number should NOT match
        assert!(!matches_item_title("Milestone 2 - Prototype", &item));
        assert!(!matches_item_title("Assignment 1", &item));
    }

    #[test]
    fn test_apply_reconciliation_updates() {
        let offset = FixedOffset::west_opt(4 * 3600).unwrap();
        let orig_due = DateTime::parse_from_rfc3339("2026-10-09T23:59:00-04:00").unwrap();
        let new_due = DateTime::parse_from_rfc3339("2026-10-14T23:59:00Z").unwrap();

        let mut vault = CourseVault {
            course_code: "CIS*4300".to_string(),
            course_name: "HCI".to_string(),
            instructor: None,
            color: None,
            links: None,
            policies: None,
            items: vec![
                DeadlineItem {
                    id: "cis4300-m1".to_string(),
                    title: "Milestone 1".to_string(),
                    item_type: ItemType::ProjectMilestone,
                    due_date: orig_due,
                    weight: 10.0,
                    status: ItemStatus::Todo,
                    submission_platform: None,
                    lead_time_days: None,
                    notes: None,
                },
                DeadlineItem {
                    id: "cis4300-a1".to_string(),
                    title: "Assignment 1".to_string(),
                    item_type: ItemType::Assignment,
                    due_date: orig_due,
                    weight: 10.0,
                    status: ItemStatus::Todo,
                    submission_platform: None,
                    lead_time_days: None,
                    notes: None,
                },
            ],
        };

        let report = ReconciliationReport {
            course_code: "CIS*4300".to_string(),
            org_unit_id: 1073361,
            checked_at: Utc::now(),
            discrepancies: vec![
                Discrepancy::DueDateShift {
                    item_id: "cis4300-m1".to_string(),
                    item_title: "Milestone 1".to_string(),
                    vault_due_date: orig_due,
                    lms_due_date: new_due,
                },
                Discrepancy::SubmittedAutoCompleted {
                    item_id: "cis4300-a1".to_string(),
                    item_title: "Assignment 1".to_string(),
                    submission_time: "2026-10-08T14:30:00Z".to_string(),
                    file_count: 1,
                },
            ],
            announcements_for_ai: Vec::new(),
        };

        let updated = apply_reconciliation_updates(&mut vault, &report);
        assert_eq!(updated, 2);

        // Check M1 due date was shifted while preserving -04:00 offset
        let m1 = vault.items.iter().find(|i| i.id == "cis4300-m1").unwrap();
        assert_eq!(m1.due_date.timezone(), offset);
        assert_eq!(m1.due_date.to_rfc3339(), "2026-10-14T19:59:00-04:00");

        // Check A1 status was completed
        let a1 = vault.items.iter().find(|i| i.id == "cis4300-a1").unwrap();
        assert_eq!(a1.status, ItemStatus::Completed);
    }
}
