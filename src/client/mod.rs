pub mod endpoints;

use crate::auth::StoredToken;
use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, ORIGIN, REFERER, USER_AGENT};
use reqwest::{Client, Response, StatusCode};
use std::time::Duration;

#[derive(Clone)]
pub struct D2LClient {
    client: Client,
    pub host: String,
    pub token: StoredToken,
}

impl D2LClient {
    pub fn new(host: &str, token: StoredToken) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token.access_token))?,
        );
        headers.insert(ORIGIN, HeaderValue::from_str(&format!("https://{}", host))?);
        headers.insert(REFERER, HeaderValue::from_str(&format!("https://{}/", host))?);
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36"),
        );
        headers.insert(ACCEPT, HeaderValue::from_static("application/json, text/plain, */*"));

        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            client,
            host: host.to_string(),
            token,
        })
    }

    pub async fn get_resilient(&self, url: &str) -> Result<Response> {
        let mut retries = 0;
        loop {
            let res = self.client.get(url).send().await?;
            if res.status() == StatusCode::TOO_MANY_REQUESTS && retries < 3 {
                let wait_secs = res
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(5);

                eprintln!("Rate limited (429). Retrying after {}s...", wait_secs);
                tokio::time::sleep(Duration::from_secs(wait_secs)).await;
                retries += 1;
                continue;
            }
            return res.error_for_status().with_context(|| format!("Request failed: {}", url));
        }
    }
}
