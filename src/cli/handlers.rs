use crate::client::D2LClient;
use crate::config::AppConfig;
use crate::reconcile::{apply_reconciliation_updates, build_reconciliation_report};
use crate::vault::resolver::resolve_course_mappings;
use crate::vault::{load_course_vault, save_course_vault};
use anyhow::Result;
use colored::*;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::Table;
use serde::Serialize;
use std::path::PathBuf;

pub async fn resolve_target_org_ids(
    client: &D2LClient,
    config: &AppConfig,
    course_query: Option<&str>,
) -> Result<Vec<(String, i64)>> {
    let enrollments = client.my_enrollments(true).await.unwrap_or_default();
    let mappings = resolve_course_mappings(&config.vault_path, &enrollments);

    if let Some(query) = course_query {
        let q = query.trim();
        // 1. Direct numeric ID (must match an actual enrollment ID or be a large OrgUnit ID >= 100,000)
        if let Ok(id) = q.parse::<i64>() {
            if enrollments.iter().any(|e| e.org_unit.id == id) || id >= 100_000 {
                return Ok(vec![(q.to_string(), id)]);
            }
        }

        // 2. Vault mappings (exact or delimiter-stripped or substring)
        let q_clean = q.replace(['*', '_', ' ', '-', '.'], "").to_uppercase();
        if let Some(m) = mappings.iter().find(|m| {
            let m_clean = m.course_code.replace(['*', '_', ' ', '-', '.'], "").to_uppercase();
            m.course_code.eq_ignore_ascii_case(q)
                || m_clean == q_clean
                || m_clean.contains(&q_clean)
        }) {
            return Ok(vec![(m.course_code.clone(), m.org_unit_id)]);
        }

        // 3. Search enrollments (code or name) - prioritize Course Offering (Type 3)
        if let Some(enr) = enrollments.iter().find(|e| {
            let is_offering = e.org_unit.unit_type.id == 3;
            let code = e.org_unit.code.as_deref().unwrap_or("").replace(['*', '_', ' ', '-', '.'], "").to_uppercase();
            let name = e.org_unit.name.replace(['*', '_', ' ', '-', '.'], "").to_uppercase();
            is_offering && (code.contains(&q_clean) || name.contains(&q_clean))
        }).or_else(|| {
            enrollments.iter().find(|e| {
                let code = e.org_unit.code.as_deref().unwrap_or("").replace(['*', '_', ' ', '-', '.'], "").to_uppercase();
                let name = e.org_unit.name.replace(['*', '_', ' ', '-', '.'], "").to_uppercase();
                code.contains(&q_clean) || name.contains(&q_clean)
            })
        }) {
            let label = enr.org_unit.code.clone().unwrap_or_else(|| enr.org_unit.name.clone());
            return Ok(vec![(label, enr.org_unit.id)]);
        }

        anyhow::bail!("Could not resolve course identifier: '{}'", query);
    } else if !mappings.is_empty() {
        Ok(mappings.into_iter().map(|m| (m.course_code, m.org_unit_id)).collect())
    } else {
        Ok(enrollments
            .into_iter()
            .map(|e| {
                let label = e.org_unit.code.unwrap_or_else(|| e.org_unit.name.clone());
                (label, e.org_unit.id)
            })
            .collect())
    }
}

pub async fn handle_status(client: &D2LClient, json_mode: bool) -> Result<()> {
    let whoami = client.whoami().await?;
    if json_mode {
        println!("{}", serde_json::to_string_pretty(&whoami)?);
    } else {
        println!("{}", "🔒 D2L Authentication Active".green().bold());
        println!("User: {} {} ({})", whoami.first_name, whoami.last_name, whoami.unique_name.cyan());
        println!("User ID: {}", whoami.identifier);
        println!(
            "Token Expires In: {:.1} min",
            (client.token.expires_at - chrono::Utc::now().timestamp()) as f64 / 60.0
        );
    }
    Ok(())
}

pub async fn handle_courses(client: &D2LClient, active_only: bool, json_mode: bool) -> Result<()> {
    let enrollments = client.my_enrollments(active_only).await?;
    if json_mode {
        println!("{}", serde_json::to_string_pretty(&enrollments)?);
    } else {
        let mut table = Table::new();
        table.apply_modifier(UTF8_ROUND_CORNERS);
        table.set_header(vec!["OrgUnit ID", "Code", "Course Name", "Active"]);

        for enr in enrollments {
            table.add_row(vec![
                enr.org_unit.id.to_string(),
                enr.org_unit.code.unwrap_or_else(|| "-".to_string()),
                enr.org_unit.name,
                if enr.access.is_active {
                    "Yes".green().to_string()
                } else {
                    "No".red().to_string()
                },
            ]);
        }
        println!("{table}");
    }
    Ok(())
}

pub async fn handle_announcements(
    client: &D2LClient,
    config: &AppConfig,
    course: Option<String>,
    since: Option<String>,
    json_mode: bool,
) -> Result<()> {
    let targets = resolve_target_org_ids(client, config, course.as_deref()).await?;

    let mut all_news = Vec::new();
    for (code, org_id) in targets {
        if let Ok(news) = client.news(org_id, since.as_deref()).await {
            for n in news {
                all_news.push((code.clone(), n));
            }
        }
    }

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&all_news)?);
    } else {
        for (code, n) in all_news {
            println!("--------------------------------------------------");
            println!("📢 [{}] {}", code.cyan().bold(), n.title.yellow().bold());
            if let Some(date) = n.start_date {
                println!("Posted: {}", date);
            }
            println!("\n{}\n", n.body.to_plain_markdown());
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct AssignmentSummary {
    course: String,
    folder_id: i64,
    name: String,
    due_date: Option<String>,
    points: Option<f64>,
    submissions_count: usize,
    last_submission_date: Option<String>,
}

pub async fn handle_assignments(
    client: &D2LClient,
    config: &AppConfig,
    course: Option<String>,
    json_mode: bool,
) -> Result<()> {
    let targets = resolve_target_org_ids(client, config, course.as_deref()).await?;
    let mut summaries = Vec::new();

    for (code, org_id) in targets {
        if let Ok(folders) = client.dropbox_folders(org_id).await {
            for f in folders {
                let subs = client.submissions(org_id, f.id).await.unwrap_or_default();
                let last_sub = subs.first().map(|s| s.submission_date.clone());
                summaries.push(AssignmentSummary {
                    course: code.clone(),
                    folder_id: f.id,
                    name: f.name,
                    due_date: f.due_date,
                    points: f.total_points,
                    submissions_count: subs.len(),
                    last_submission_date: last_sub,
                });
            }
        }
    }

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&summaries)?);
    } else {
        let mut table = Table::new();
        table.apply_modifier(UTF8_ROUND_CORNERS);
        table.set_header(vec![
            "Course",
            "Folder ID",
            "Assignment Name",
            "Due Date",
            "Points",
            "Submitted",
        ]);

        for s in summaries {
            let submitted_display = if s.submissions_count > 0 {
                format!("✅ Yes ({})", s.submissions_count).green().to_string()
            } else {
                "❌ No".yellow().to_string()
            };

            table.add_row(vec![
                s.course,
                s.folder_id.to_string(),
                s.name,
                s.due_date.unwrap_or_else(|| "-".to_string()),
                s.points.map(|p| p.to_string()).unwrap_or_else(|| "-".to_string()),
                submitted_display,
            ]);
        }
        println!("{table}");
    }
    Ok(())
}

pub async fn handle_content(
    client: &D2LClient,
    config: &AppConfig,
    course: Option<String>,
    json_mode: bool,
) -> Result<()> {
    let targets = resolve_target_org_ids(client, config, course.as_deref()).await?;
    let mut all_tocs = Vec::new();

    for (code, org_id) in targets {
        if let Ok(toc) = client.content_toc(org_id).await {
            all_tocs.push((code, org_id, toc));
        }
    }

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&all_tocs)?);
    } else {
        for (code, org_id, toc) in all_tocs {
            println!("📚 Content for {} (OrgUnit {}):", code, org_id);
            for m in toc.modules {
                println!("  📁 {}", m.title);
                if let Some(ref d) = m.description {
                    if let Some(ref txt) = d.text {
                        let preview: String = txt.lines().take(3).collect::<Vec<_>>().join(" ");
                        if !preview.trim().is_empty() {
                            println!("     📝 {}", preview.trim());
                        }
                    }
                }
                for t in m.topics {
                    println!("    📄 {} ({})", t.title, t.url.unwrap_or_default());
                }
            }
        }
    }
    Ok(())
}

pub async fn handle_quizzes(
    client: &D2LClient,
    config: &AppConfig,
    course: Option<String>,
    json_mode: bool,
) -> Result<()> {
    let targets = resolve_target_org_ids(client, config, course.as_deref()).await?;
    let mut all_quizzes = Vec::new();

    for (code, org_id) in targets {
        if let Ok(quizzes) = client.quizzes(org_id).await {
            for q in quizzes {
                all_quizzes.push((code.clone(), q));
            }
        }
    }

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&all_quizzes)?);
    } else {
        let mut table = Table::new();
        table.apply_modifier(UTF8_ROUND_CORNERS);
        table.set_header(vec!["Course", "Quiz ID", "Name", "Due Date", "Time Limit (min)"]);

        for (code, q) in all_quizzes {
            table.add_row(vec![
                code,
                q.quiz_id.to_string(),
                q.name,
                q.due_date.unwrap_or_else(|| "-".to_string()),
                q.time_limit.map(|t| t.to_string()).unwrap_or_else(|| "None".to_string()),
            ]);
        }
        println!("{table}");
    }
    Ok(())
}

pub async fn handle_grades(
    client: &D2LClient,
    config: &AppConfig,
    course: Option<String>,
    json_mode: bool,
) -> Result<()> {
    let targets = resolve_target_org_ids(client, config, course.as_deref()).await?;
    let mut all_grades = Vec::new();

    for (code, org_id) in targets {
        if let Ok(grades) = client.grades(org_id).await {
            for g in grades {
                all_grades.push((code.clone(), g));
            }
        }
    }

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&all_grades)?);
    } else {
        let mut table = Table::new();
        table.apply_modifier(UTF8_ROUND_CORNERS);
        table.set_header(vec!["Course", "Grade Item", "Points", "Weight %", "Comments"]);

        for (code, g) in all_grades {
            let pts_display = match (g.points_numerator, g.points_denominator) {
                (Some(num), Some(den)) => format!("{:.1} / {:.1}", num, den),
                (Some(num), None) => format!("{:.1}", num),
                _ => "-".to_string(),
            };
            let weight_display = match (g.weighted_numerator, g.weighted_denominator) {
                (Some(num), Some(den)) => format!("{:.1} / {:.1}%", num, den),
                (Some(num), None) => format!("{:.1}%", num),
                _ => "-".to_string(),
            };
            let comment_text = g.comments.map(|c| c.to_plain_markdown()).unwrap_or_default();

            table.add_row(vec![code, g.name, pts_display, weight_display, comment_text]);
        }
        println!("{table}");
    }
    Ok(())
}

pub async fn handle_calendar(
    client: &D2LClient,
    config: &AppConfig,
    days: u32,
    json_mode: bool,
) -> Result<()> {
    let targets = resolve_target_org_ids(client, config, None).await?;
    let org_ids_csv = targets
        .iter()
        .map(|(_, id)| id.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let events = if !org_ids_csv.is_empty() {
        client.calendar_events(&org_ids_csv).await.unwrap_or_default()
    } else {
        Vec::new()
    };

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&events)?);
    } else {
        println!("📅 Upcoming Calendar Events (Next {} Days):", days);
        let mut table = Table::new();
        table.apply_modifier(UTF8_ROUND_CORNERS);
        table.set_header(vec!["Start Time", "Title", "Location"]);

        for ev in events {
            table.add_row(vec![
                ev.start_date_time,
                ev.title,
                ev.location.unwrap_or_else(|| "-".to_string()),
            ]);
        }
        println!("{table}");
    }
    Ok(())
}

pub async fn handle_posts(
    client: &D2LClient,
    config: &AppConfig,
    course: String,
    forum: Option<i64>,
    topic: Option<i64>,
    json_mode: bool,
) -> Result<()> {
    let targets = resolve_target_org_ids(client, config, Some(&course)).await?;
    let (_, org_id) = targets
        .first()
        .ok_or_else(|| anyhow::anyhow!("Could not resolve course '{}'", course))?;

    if let (Some(f_id), Some(t_id)) = (forum, topic) {
        let posts = client.posts(*org_id, f_id, t_id).await?;
        if json_mode {
            println!("{}", serde_json::to_string_pretty(&posts)?);
        } else {
            for p in posts {
                println!("--------------------------------------------------");
                println!("📝 Subject: {}", p.subject.cyan().bold());
                println!("Posted: {}", p.date_posted);
                println!("\n{}\n", p.message.to_plain_markdown());
            }
        }
    } else if let Some(f_id) = forum {
        let topics = client.topics(*org_id, f_id).await?;
        if json_mode {
            println!("{}", serde_json::to_string_pretty(&topics)?);
        } else {
            let mut table = Table::new();
            table.apply_modifier(UTF8_ROUND_CORNERS);
            table.set_header(vec!["Topic ID", "Topic Name", "Description"]);
            for t in topics {
                let desc = t.description.map(|d| d.to_plain_markdown()).unwrap_or_default();
                table.add_row(vec![t.topic_id.to_string(), t.name, desc]);
            }
            println!("{table}");
        }
    } else {
        let forums = client.forums(*org_id).await?;
        if json_mode {
            println!("{}", serde_json::to_string_pretty(&forums)?);
        } else {
            let mut table = Table::new();
            table.apply_modifier(UTF8_ROUND_CORNERS);
            table.set_header(vec!["Forum ID", "Forum Name", "Description"]);
            for f in forums {
                let desc = f.description.map(|d| d.to_plain_markdown()).unwrap_or_default();
                table.add_row(vec![f.forum_id.to_string(), f.name, desc]);
            }
            println!("{table}");
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct UnifiedCourseDump {
    course_code: String,
    org_unit_id: i64,
    announcements: Vec<crate::models::d2l::NewsItem>,
    dropboxes: Vec<crate::models::d2l::DropboxFolder>,
    quizzes: Vec<crate::models::d2l::Quiz>,
    grades: Vec<crate::models::d2l::GradeItem>,
}

#[derive(Serialize)]
struct UnifiedDump {
    user: crate::models::d2l::WhoAmIResponse,
    courses: Vec<UnifiedCourseDump>,
}

pub async fn handle_dump(
    client: &D2LClient,
    config: &AppConfig,
    course: Option<String>,
) -> Result<()> {
    let whoami = client.whoami().await?;
    let targets = resolve_target_org_ids(client, config, course.as_deref()).await?;

    let mut courses_dump = Vec::new();
    for (code, org_id) in targets {
        let news = client.news(org_id, None).await.unwrap_or_default();
        let dropboxes = client.dropbox_folders(org_id).await.unwrap_or_default();
        let quizzes = client.quizzes(org_id).await.unwrap_or_default();
        let grades = client.grades(org_id).await.unwrap_or_default();

        courses_dump.push(UnifiedCourseDump {
            course_code: code,
            org_unit_id: org_id,
            announcements: news,
            dropboxes,
            quizzes,
            grades,
        });
    }

    let dump = UnifiedDump {
        user: whoami,
        courses: courses_dump,
    };

    println!("{}", serde_json::to_string_pretty(&dump)?);
    Ok(())
}

pub async fn handle_download(
    client: &D2LClient,
    config: &AppConfig,
    course: String,
    kind: String,
    dest: Option<PathBuf>,
    _json_mode: bool,
) -> Result<()> {
    let targets = resolve_target_org_ids(client, config, Some(&course)).await?;
    let (code, org_id) = targets
        .first()
        .ok_or_else(|| anyhow::anyhow!("Could not resolve course '{}'", course))?;

    let target_dir = dest.unwrap_or_else(|| {
        let sub = match kind.to_lowercase().as_str() {
            "assignments" => "Assignments",
            "syllabus" => "Syllabus",
            _ => "Materials",
        };
        config
            .vault_path
            .join(code.replace('*', ""))
            .join(sub)
    });

    println!(
        "Downloading {} for {} (OrgUnit {}) to {:?}...",
        kind, code, org_id, target_dir
    );

    let toc = client.content_toc(*org_id).await?;
    let mut downloaded_count = 0;

    fn collect_topics(
        modules: &[crate::models::d2l::ContentModule],
        acc: &mut Vec<crate::models::d2l::ContentTopic>,
        filter_kind: &str,
    ) {
        for m in modules {
            let m_lower = m.title.to_lowercase();
            let module_matches = match filter_kind {
                "assignments" => m_lower.contains("assign"),
                "syllabus" => m_lower.contains("syllabus") || m_lower.contains("outline"),
                _ => true,
            };

            for t in &m.topics {
                let t_lower = t.title.to_lowercase();
                let topic_matches = match filter_kind {
                    "assignments" => {
                        module_matches
                            || t_lower.contains("assign")
                            || t_lower.contains("a1")
                            || t_lower.contains("a2")
                            || t_lower.contains("a3")
                            || t_lower.contains("a4")
                    }
                    "syllabus" => {
                        module_matches || t_lower.contains("syllabus") || t_lower.contains("outline")
                    }
                    _ => true,
                };
                if topic_matches {
                    acc.push(t.clone());
                }
            }
            collect_topics(&m.modules, acc, filter_kind);
        }
    }

    let mut topics = Vec::new();
    collect_topics(&toc.modules, &mut topics, &kind.to_lowercase());

    for t in topics {
        if let Some(ref url) = t.url {
            let lower = url.to_lowercase();
            if lower.ends_with(".pdf")
                || lower.ends_with(".docx")
                || lower.ends_with(".zip")
                || lower.ends_with(".ipynb")
                || lower.ends_with(".py")
                || lower.ends_with(".c")
                || lower.ends_with(".cpp")
                || lower.ends_with(".java")
                || lower.ends_with(".tar.gz")
                || lower.ends_with(".csv")
            {
                let full_url = if url.starts_with("http") {
                    url.clone()
                } else {
                    format!("https://{}{}", client.host, url)
                };
                if let Ok(saved) = client.download_stream_to_file(&full_url, &target_dir).await {
                    println!("  💾 Downloaded: {}", saved);
                    downloaded_count += 1;
                }
            }
        }
    }

    println!("✅ Downloaded {} files to {:?}", downloaded_count, target_dir);
    Ok(())
}

pub async fn handle_reconcile(
    client: &D2LClient,
    config: &AppConfig,
    course_filter: Option<String>,
    apply: bool,
    json_mode: bool,
) -> Result<()> {
    let enrollments = client.my_enrollments(true).await.unwrap_or_default();
    let mappings = resolve_course_mappings(&config.vault_path, &enrollments);

    let mut reports = Vec::new();

    for m in mappings {
        if let Some(ref cf) = course_filter {
            let cf_clean = cf.replace('*', "").to_uppercase();
            let m_clean = m.course_code.replace('*', "").to_uppercase();
            if !m.course_code.eq_ignore_ascii_case(cf) && m_clean != cf_clean {
                continue;
            }
        }

        if let Some(ref path) = m.file_path {
            if let Ok(mut vault) = load_course_vault(path) {
                if let Ok(report) =
                    build_reconciliation_report(client, &m.course_code, m.org_unit_id, &vault).await
                {
                    if apply {
                        let updated = apply_reconciliation_updates(&mut vault, &report);
                        if updated > 0 {
                            save_course_vault(path, &vault)?;
                            eprintln!("Applied {} updates to {:?}", updated, path);
                        }
                    }
                    reports.push(report);
                }
            }
        }
    }

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&reports)?);
    } else {
        for r in reports {
            println!(
                "=== Reconciliation for {} (OrgUnit {}) ===",
                r.course_code.cyan().bold(),
                r.org_unit_id
            );
            if r.discrepancies.is_empty() {
                println!("{}", "  ✨ All vault deadlines match live D2L state!".green());
            } else {
                for d in r.discrepancies {
                    match d {
                        crate::models::reconcile::Discrepancy::DueDateShift {
                            item_title,
                            vault_due_date,
                            lms_due_date,
                            ..
                        } => {
                            println!(
                                "  🔄 Shift: {} (Vault: {} -> LMS: {})",
                                item_title.yellow(),
                                vault_due_date,
                                lms_due_date
                            );
                        }
                        crate::models::reconcile::Discrepancy::SubmittedAutoCompleted {
                            item_title,
                            submission_time,
                            ..
                        } => {
                            println!(
                                "  ✅ Submitted: {} (At: {}) -> Can mark completed",
                                item_title.green(),
                                submission_time
                            );
                        }
                        crate::models::reconcile::Discrepancy::MissingInVault {
                            lms_title,
                            lms_due_date,
                            ..
                        } => {
                            println!(
                                "  ⚠️ Missing in Vault: {} (Due: {:?})",
                                lms_title.red(),
                                lms_due_date
                            );
                        }
                    }
                }
            }
            if !r.announcements_for_ai.is_empty() {
                println!(
                    "  📢 {} recent announcements available for AI prompt reconciliation.",
                    r.announcements_for_ai.len()
                );
            }
            println!();
        }
    }
    Ok(())
}
