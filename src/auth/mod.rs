pub mod token;
pub mod cdp;

pub use token::StoredToken;
use crate::config::AppConfig;
use anyhow::Result;

pub async fn resolve_token(config: &AppConfig, force_refresh: bool, headless_first: bool) -> Result<StoredToken> {
    // 1. Direct environment override
    if let Ok(env_tok) = std::env::var("D2L_TOKEN") {
        if !env_tok.trim().is_empty() {
            return StoredToken::from_jwt(&config.host, env_tok.trim());
        }
    }

    // 2. Cached token check
    if !force_refresh {
        if let Some(token) = StoredToken::load_from_file(&config.token_path)? {
            if token.is_valid() {
                return Ok(token);
            }
        }
    }

    // 3. Automated browser acquisition
    config.ensure_dirs()?;

    if headless_first {
        eprintln!("Attempting headless session restore with saved cookies...");
        if let Ok(token) = cdp::intercept_d2l_token(&config.host, &config.browser_profile_dir, true, 8).await {
            token.save_to_file(&config.token_path)?;
            return Ok(token);
        }
        eprintln!("Headless restore expired or 2FA required. Launching browser window for login...");
    }

    let token = cdp::intercept_d2l_token(&config.host, &config.browser_profile_dir, false, 180).await?;
    token.save_to_file(&config.token_path)?;
    Ok(token)
}
