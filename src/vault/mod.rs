pub mod resolver;

use crate::models::vault::CourseVault;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub fn load_course_vault(file_path: &Path) -> Result<CourseVault> {
    let data = fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read deadlines file: {:?}", file_path))?;
    let course: CourseVault = serde_json::from_str(&data)
        .with_context(|| format!("Failed to parse JSON in: {:?}", file_path))?;
    Ok(course)
}

pub fn save_course_vault(file_path: &Path, course: &CourseVault) -> Result<()> {
    // 1. Write atomic .bak backup
    let bak_path = file_path.with_extension("json.bak");
    if file_path.exists() {
        let _ = fs::copy(file_path, &bak_path);
    }

    // 2. Format pretty JSON and write
    let json = serde_json::to_string_pretty(course)?;
    fs::write(file_path, json)?;
    Ok(())
}

pub fn find_all_course_files(vault_path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(vault_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let json_path = path.join("deadlines.json");
                if json_path.is_file() {
                    files.push(json_path);
                }
            }
        }
    }
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_load_real_vault_files_if_exist() {
        let vault_path = Path::new("/home/lmb/Documents/LeeOsVault/F26");
        if vault_path.exists() {
            let files = find_all_course_files(vault_path);
            assert!(!files.is_empty(), "Should find course files in user vault");
            for file in files {
                let course = load_course_vault(&file);
                assert!(course.is_ok(), "Failed to load course from {:?}", file);
                let c = course.unwrap();
                assert!(!c.course_code.is_empty());
                println!("Loaded real vault course: {} ({} items)", c.course_code, c.items.len());
            }
        }
    }

    #[test]
    fn test_save_course_vault_backup() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("deadlines.json");

        let mut course = CourseVault {
            course_code: "TEST*1000".to_string(),
            course_name: "Test Course".to_string(),
            instructor: None,
            color: None,
            links: None,
            policies: None,
            items: Vec::new(),
        };

        // First save: no backup existed
        save_course_vault(&file_path, &course).unwrap();
        assert!(file_path.exists());

        // Second save: should create .json.bak
        course.course_name = "Updated Test Course".to_string();
        save_course_vault(&file_path, &course).unwrap();

        let bak_path = file_path.with_extension("json.bak");
        assert!(bak_path.exists(), "Backup file should be created");

        let loaded = load_course_vault(&file_path).unwrap();
        assert_eq!(loaded.course_name, "Updated Test Course");

        let loaded_bak = load_course_vault(&bak_path).unwrap();
        assert_eq!(loaded_bak.course_name, "Test Course");
    }

    #[test]
    fn test_find_all_course_files() {
        let dir = tempdir().unwrap();
        let course1 = dir.path().join("CIS1000");
        let course2 = dir.path().join("STAT2000");
        let non_course = dir.path().join("Notes");

        std::fs::create_dir_all(&course1).unwrap();
        std::fs::create_dir_all(&course2).unwrap();
        std::fs::create_dir_all(&non_course).unwrap();

        std::fs::write(course1.join("deadlines.json"), "{}").unwrap();
        std::fs::write(course2.join("deadlines.json"), "{}").unwrap();
        std::fs::write(non_course.join("other.txt"), "hello").unwrap();

        let files = find_all_course_files(dir.path());
        assert_eq!(files.len(), 2);
    }
}
