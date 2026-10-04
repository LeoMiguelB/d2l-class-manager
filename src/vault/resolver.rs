use crate::models::d2l::MyEnrollment;
use crate::vault::load_course_vault;
use regex::Regex;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct CourseMapping {
    pub course_code: String,
    pub org_unit_id: i64,
    pub file_path: Option<std::path::PathBuf>,
}

pub fn extract_org_id_from_url(url: &str) -> Option<i64> {
    let re = Regex::new(r"(?:/d2l/(?:home|le/[^/]+|lms/[^/]+)/(\d+)|[?&]ou=(\d+))").ok()?;
    let caps = re.captures(url)?;
    caps.get(1)
        .or_else(|| caps.get(2))
        .and_then(|m| m.as_str().parse::<i64>().ok())
}

pub fn resolve_course_mappings(vault_path: &Path, enrollments: &[MyEnrollment]) -> Vec<CourseMapping> {
    let course_files = crate::vault::find_all_course_files(vault_path);
    let mut mappings = Vec::new();

    for file in course_files {
        if let Ok(course) = load_course_vault(&file) {
            let mut org_id = None;
            if let Some(ref links) = course.links {
                if let Some(cl_url) = links.get("courselink") {
                    org_id = extract_org_id_from_url(cl_url);
                }
            }

            // Fallback fuzzy match against enrollments if link was missing
            if org_id.is_none() {
                let clean_code = course.course_code.replace('*', "").to_uppercase();
                for enr in enrollments {
                    let enr_code = enr.org_unit.code.as_deref().unwrap_or("").replace('*', "").to_uppercase();
                    let enr_name = enr.org_unit.name.to_uppercase();
                    if enr_code.contains(&clean_code) || enr_name.contains(&clean_code) {
                        org_id = Some(enr.org_unit.id);
                        break;
                    }
                }
            }

            if let Some(id) = org_id {
                mappings.push(CourseMapping {
                    course_code: course.course_code,
                    org_unit_id: id,
                    file_path: Some(file),
                });
            }
        }
    }
    mappings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_org_id_from_various_urls() {
        assert_eq!(
            extract_org_id_from_url("https://courselink.uoguelph.ca/d2l/home/1073361"),
            Some(1073361)
        );
        assert_eq!(
            extract_org_id_from_url("https://courselink.uoguelph.ca/d2l/home/1073361/"),
            Some(1073361)
        );
        assert_eq!(
            extract_org_id_from_url("https://courselink.uoguelph.ca/d2l/le/content/1073361/Home"),
            Some(1073361)
        );
        assert_eq!(
            extract_org_id_from_url("https://courselink.uoguelph.ca/d2l/lms/dropbox/user/folders_list.d2l?ou=1073361"),
            Some(1073361)
        );
        assert_eq!(
            extract_org_id_from_url("CourseLink"),
            None
        );
        assert_eq!(
            extract_org_id_from_url("Submissions via CourseLink"),
            None
        );
    }
}
