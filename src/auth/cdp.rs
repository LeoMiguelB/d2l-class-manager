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

    tokio::spawn(async move {
        while let Some(h) = handler.next().await {
            if h.is_err() {
                break;
            }
        }
    });

    let portal_url = format!("https://{}/d2l/home", host);
    let page = browser.new_page(&portal_url).await?;

    let mut events = page.event_listener::<EventRequestWillBeSent>().await?;
    let timeout_fut = tokio::time::sleep(Duration::from_secs(timeout_secs));
    tokio::pin!(timeout_fut);

    let mut captured_jwt = None;

    loop {
        tokio::select! {
            _ = &mut timeout_fut => {
                let _ = browser.close().await;
                anyhow::bail!("Timed out after {}s waiting for D2L Bearer token. Please run `d2l login` in headed mode.", timeout_secs);
            }
            Some(event) = events.next() => {
                if let Some(headers) = event.request.headers.inner().as_object() {
                    for (k, v) in headers {
                        if k.eq_ignore_ascii_case("authorization") {
                            if let Some(val) = v.as_str() {
                                if val.starts_with("Bearer eyJ") {
                                    let jwt = val.trim_start_matches("Bearer ").trim();
                                    captured_jwt = Some(jwt.to_string());
                                    break;
                                }
                            }
                        }
                    }
                }
                if captured_jwt.is_some() {
                    break;
                }
            }
        }
    }

    let _ = browser.close().await;

    let jwt = captured_jwt.context("Failed to capture Bearer token from network events")?;
    StoredToken::from_jwt(host, &jwt)
}
