use crate::auth::token::StoredToken;
use anyhow::{Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::network::EventRequestWillBeSent;
use futures_util::StreamExt;
use std::path::Path;
use std::time::Duration;

pub fn find_chrome_binary() -> Result<String> {
    if let Ok(path) = std::env::var("CHROME_BIN") {
        if Path::new(&path).exists() {
            return Ok(path);
        }
    }
    let candidates = [
        "/usr/bin/google-chrome-stable",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
    ];
    for candidate in candidates {
        if Path::new(candidate).exists() {
            return Ok(candidate.to_string());
        }
    }
    anyhow::bail!("No Chrome/Chromium executable found. Please install google-chrome or set CHROME_BIN.")
}

pub async fn intercept_d2l_token(
    host: &str,
    profile_dir: &Path,
    headless: bool,
    timeout_secs: u64,
) -> Result<StoredToken> {
    let chrome_bin = find_chrome_binary()?;
    let mut builder = BrowserConfig::builder()
        .chrome_executable(chrome_bin)
        .user_data_dir(profile_dir)
        .arg("--disable-gpu")
        .arg("--no-first-run");

    if !headless {
        builder = builder.with_head();
    }

    let config = builder.build().map_err(|e| anyhow::anyhow!("{}", e))?;
    let (mut browser, mut handler) = Browser::launch(config).await?;

    let (exit_tx, mut exit_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        while let Some(h) = handler.next().await {
            if h.is_err() {
                break;
            }
        }
        let _ = exit_tx.send(());
    });

    let portal_url = format!("https://{}/d2l/home", host);
    let page = browser.new_page(&portal_url).await?;

    let mut events = page.event_listener::<EventRequestWillBeSent>().await?;
    let mut check_interval = tokio::time::interval(Duration::from_millis(500));
    let start_time = std::time::Instant::now();
    let max_duration = Duration::from_secs(timeout_secs);

    let mut captured_token: Option<StoredToken> = None;

    loop {
        if start_time.elapsed() > max_duration {
            let _ = tokio::time::timeout(Duration::from_millis(1000), browser.close()).await;
            anyhow::bail!("Timed out after {}s waiting for D2L login to complete.", timeout_secs);
        }

        tokio::select! {
            _ = &mut exit_rx => {
                anyhow::bail!("Browser was closed before login was completed.");
            }
            Some(event) = events.next() => {
                if let Some(headers) = event.request.headers.inner().as_object() {
                    for (k, v) in headers {
                        if k.eq_ignore_ascii_case("authorization") {
                            if let Some(val) = v.as_str() {
                                if val.starts_with("Bearer eyJ") {
                                    let jwt = val.trim_start_matches("Bearer ").trim();
                                    captured_token = Some(StoredToken::from_jwt(host, jwt)?);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            _ = check_interval.tick() => {
                // If on login page, automatically click SSO sign-in button if present
                let _ = page.evaluate("() => { const sso = document.getElementById('sso'); if (sso) sso.click(); }").await;

                // Check cookies directly across the browser session
                if let Ok(cookies) = browser.get_cookies().await {
                    let has_session_val = cookies.iter().any(|c| c.name == "d2lSessionVal");
                    let has_secure_val = cookies.iter().any(|c| c.name == "d2lSecureSessionVal");

                    if has_session_val && has_secure_val {
                        // Extract username from localStorage if present (no network call)
                        let username: Option<String> = page
                            .evaluate("() => localStorage.getItem('userNameD2L')")
                            .await
                            .ok()
                            .and_then(|v| v.into_value::<Option<String>>().ok())
                            .flatten();

                        let cookie_parts: Vec<String> = cookies
                            .iter()
                            .filter(|c| c.name.starts_with("d2l") || c.name == "cookiesession1")
                            .map(|c| format!("{}={}", c.name, c.value))
                            .collect();

                        let cookie_str = cookie_parts.join("; ");
                        captured_token = Some(StoredToken::from_cookies(host, &cookie_str, username.as_deref()));
                    }
                }
            }
        }

        if captured_token.is_some() {
            break;
        }
    }

    let _ = tokio::time::timeout(Duration::from_millis(1000), browser.close()).await;

    captured_token.context("Failed to capture authentication session")
}
