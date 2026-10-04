use anyhow::{Context, Result};
use std::path::PathBuf;

pub const DEFAULT_HOST: &str = "courselink.uoguelph.ca";
pub const DEFAULT_VAULT_PATH: &str = "/home/lmb/Documents/LeeOsVault/F26";

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub host: String,
    pub config_dir: PathBuf,
    pub token_path: PathBuf,
    pub browser_profile_dir: PathBuf,
    #[allow(dead_code)]
    pub courses_cache_path: PathBuf,
    pub vault_path: PathBuf,
}

impl AppConfig {
    pub fn new(host_override: Option<String>, vault_override: Option<PathBuf>) -> Result<Self> {
        let host = host_override
            .or_else(|| std::env::var("D2L_HOST").ok())
            .unwrap_or_else(|| DEFAULT_HOST.to_string());

        let base_dir = dirs::config_dir()
            .context("Failed to resolve user config directory")?
            .join("d2l-manager");

        let token_path = base_dir.join("token.json");
        let browser_profile_dir = base_dir.join("browser_profile");
        let courses_cache_path = base_dir.join("courses.json");

        let vault_path = vault_override
            .or_else(|| std::env::var("SCHOOL_DASHBOARD_VAULT").ok().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(DEFAULT_VAULT_PATH));

        Ok(Self {
            host,
            config_dir: base_dir,
            token_path,
            browser_profile_dir,
            courses_cache_path,
            vault_path,
        })
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.config_dir)
            .with_context(|| format!("Failed to create config dir: {:?}", self.config_dir))?;
        std::fs::create_dir_all(&self.browser_profile_dir)
            .with_context(|| format!("Failed to create profile dir: {:?}", self.browser_profile_dir))?;
        Ok(())
    }
}
