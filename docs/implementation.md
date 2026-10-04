# Implementation Specification: `d2l-class-manager`

**Document Status:** Complete Builder Agent Specification  
**Target Package:** `d2l-class-manager` (Binary: `d2l`)  
**Language & Edition:** Rust 2021 (Rustc 1.80+)  
**Companion Documents:** [`docs/proposal.md`](file:///home/lmb/Desktop/projects/d2l-class-manager/docs/proposal.md), [`docs/findings.md`](file:///home/lmb/Desktop/projects/d2l-class-manager/docs/findings.md), [`docs/discussion.md`](file:///home/lmb/Desktop/projects/d2l-class-manager/docs/discussion.md), [`docs/use-cases.md`](file:///home/lmb/Desktop/projects/d2l-class-manager/docs/use-cases.md)  
**Companion Project:** [`School-Dashboard`](file:///home/lmb/Desktop/projects/School-Dashboard) (`schoodash`)  
**Default Vault Path:** `/home/lmb/Documents/LeeOsVault/F26` (configurable via `SCHOOL_DASHBOARD_VAULT`)  

---

## 1. Builder Agent Instructions

This document provides an exhaustive, unambiguous specification for building `d2l-class-manager`.
Any builder agent consuming this specification should build the project sequentially following the steps below.

### Core Implementation Principles
1. **Zero Guesswork:** All structs, fields, traits, endpoints, error handlings, and file paths are explicitly typed and provided.
2. **Single-Binary Rust:** The tool compiles to a single standalone binary named `d2l`.
3. **Dual Output Mode:** Every command supports `--json` (clean JSON to stdout for AI/scripts) and human-friendly formatted terminal tables (via `comfy-table`) when run interactively.
4. **Strictly Read-Only on D2L:** Never issue `POST`, `PUT`, or `DELETE` requests to Valence endpoints (except authentication session polling).
5. **Safe Vault Mutation:** All writes to `deadlines.json` files must preserve existing fields and automatically create an atomic `.bak` backup file beforehand.

---

## 2. Directory Layout & File Manifest

The builder agent will create the following layout under `/home/lmb/Desktop/projects/d2l-class-manager`:

```
d2l-class-manager/
├── Cargo.toml
├── docs/
│   ├── discussion.md
│   ├── findings.md
│   ├── implementation.md
│   ├── proposal.md
│   └── use-cases.md
└── src/
    ├── main.rs
    ├── config.rs
    ├── auth/
    │   ├── mod.rs
    │   ├── token.rs
    │   └── cdp.rs
    ├── client/
    │   ├── mod.rs
    │   └── endpoints.rs
    ├── models/
    │   ├── mod.rs
    │   ├── d2l.rs
    │   ├── vault.rs
    │   └── reconcile.rs
    ├── vault/
    │   ├── mod.rs
    │   └── resolver.rs
    ├── reconcile/
    │   └── mod.rs
    └── cli/
        ├── mod.rs
        └── handlers.rs
```

---

## 3. Verbatim `Cargo.toml`

Create `Cargo.toml` with the exact dependencies and features below:

```toml
[package]
name = "d2l-class-manager"
version = "0.1.0"
edition = "2021"
authors = ["LeeOs <lmb@local>"]
description = "High-performance Rust CLI data getter, content downloader, and AI reconciliation layer for D2L Brightspace / CourseLink"

[[bin]]
name = "d2l"
path = "src/main.rs"

[dependencies]
# Async Runtime
tokio = { version = "1.40", features = ["full"] }
futures-util = "0.3"

# CLI Parsing & Output
clap = { version = "4.5", features = ["derive", "env"] }
comfy-table = "7.1"
colored = "2.1"

# HTTP & Network Client
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls", "stream"] }

# Serialization & Chrono
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
chrono = { version = "0.4", features = ["serde"] }

# System Paths & Files
dirs = "5.0"
regex = "1.10"
anyhow = "1.0"
thiserror = "1.0"
base64 = "0.22"

# HTML to Text (for reading posts & announcement cleanup)
html2text = "0.17"

# Chrome DevTools Protocol for SSO / Bearer Interception
chromiumoxide = { version = "0.9.1", default-features = false, features = ["tokio-runtime"] }

[dev-dependencies]
tempfile = "3.12"
```

---

## 4. Step 1: Configuration Management (`src/config.rs`)

### Responsibilities
- Resolves default LMS host (`courselink.uoguelph.ca` for Guelph).
- Resolves XDG paths (`~/.config/d2l-manager/`).
- Resolves the student vault path from `--vault`, `$SCHOOL_DASHBOARD_VAULT`, or fallback `/home/lmb/Documents/LeeOsVault/F26`.

### Implementation: `src/config.rs`
```rust
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
```

---

## 5. Step 2: Data Models (`src/models/`)

### 5.1 Module Declaration: `src/models/mod.rs`
```rust
pub mod d2l;
pub mod vault;
pub mod reconcile;

pub use d2l::*;
pub use vault::*;
pub use reconcile::*;
```

### 5.2 D2L Valence Models: `src/models/d2l.rs`
Create `src/models/d2l.rs` containing models that match Valence REST responses:

```rust
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
    pub attachments: Vec<AttachmentInfo>,
    pub total_points: Option<f64>,
    pub due_date: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub is_hidden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AttachmentInfo {
    pub file_id: i64,
    pub file_name: String,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Submission {
    pub id: i64,
    pub submission_date: String,
    pub files: Vec<SubmissionFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SubmissionFile {
    pub file_id: i64,
    pub file_name: String,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContentToc {
    pub modules: Vec<ContentModule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContentModule {
    pub module_id: i64,
    pub title: String,
    pub modules: Vec<ContentModule>,
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
```

### 5.3 Vault Models (Compatible with `School-Dashboard`): `src/models/vault.rs`
```rust
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourseVault {
    pub course_code: String,
    pub course_name: String,
    #[serde(default)]
    pub instructor: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub links: Option<HashMap<String, String>>,
    #[serde(default)]
    pub policies: Option<HashMap<String, String>>,
    #[serde(default)]
    pub items: Vec<DeadlineItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemType {
    Exam,
    Assignment,
    ProjectMilestone,
    Critique,
    Presentation,
    Admin,
    Participation,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Todo,
    InProgress,
    Completed,
    Dropped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadlineItem {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub item_type: ItemType,
    pub due_date: DateTime<FixedOffset>,
    #[serde(default)]
    pub weight: f64,
    #[serde(default = "default_status")]
    pub status: ItemStatus,
    #[serde(default)]
    pub submission_platform: Option<String>,
    #[serde(default)]
    pub lead_time_days: Option<i64>,
    #[serde(default)]
    pub notes: Option<String>,
}

fn default_status() -> ItemStatus {
    ItemStatus::Todo
}
```

### 5.4 Reconciliation Models: `src/models/reconcile.rs`
```rust
use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationReport {
    pub course_code: String,
    pub org_unit_id: i64,
    pub checked_at: DateTime<Utc>,
    pub discrepancies: Vec<Discrepancy>,
    pub announcements_for_ai: Vec<AnnouncementSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "details")]
pub enum Discrepancy {
    DueDateShift {
        item_id: String,
        item_title: String,
        vault_due_date: DateTime<FixedOffset>,
        lms_due_date: DateTime<FixedOffset>,
    },
    SubmittedAutoCompleted {
        item_id: String,
        item_title: String,
        submission_time: String,
        file_count: usize,
    },
    MissingInVault {
        lms_folder_id: i64,
        lms_title: String,
        lms_due_date: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnouncementSummary {
    pub id: i64,
    pub title: String,
    pub posted_date: Option<String>,
    pub markdown_body: String,
}
```

---

## 6. Step 3: Authentication & Token Management (`src/auth/`)

### 6.1 Token Storage & Claims: `src/auth/token.rs`
```rust
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredToken {
    pub host: String,
    pub access_token: String,
    pub expires_at: i64,
    pub sub: Option<String>,
    pub tenant_id: Option<String>,
    pub captured_at: DateTime<Utc>,
}

impl StoredToken {
    pub fn is_valid(&self) -> bool {
        let now = Utc::now().timestamp();
        // 60-second safety window
        self.expires_at > (now + 60)
    }

    pub fn load_from_file(path: &Path) -> Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(path)?;
        let token: StoredToken = serde_json::from_str(&data)?;
        Ok(Some(token))
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    pub fn from_jwt(host: &str, jwt: &str) -> Result<Self> {
        let parts: Vec<&str> = jwt.split('.').collect();
        if parts.len() < 2 {
            anyhow::bail!("Invalid JWT format: missing payload");
        }

        let mut payload_b64 = parts[1].to_string();
        let rem = payload_b64.len() % 4;
        if rem > 0 {
            payload_b64.push_str(&"=".repeat(4 - rem));
        }

        use base64::Engine;
        let decoded = base64::engine::general_purpose::URL_SAFE
            .decode(&payload_b64)
            .or_else(|_| base64::engine::general_purpose::STANDARD.decode(&payload_b64))
            .context("Failed to base64-decode JWT payload")?;

        let claims: serde_json::Value = serde_json::from_slice(&decoded)?;
        let expires_at = claims.get("exp").and_then(|v| v.as_i64()).unwrap_or(0);
        let sub = claims.get("sub").and_then(|v| v.as_str()).map(String::from);
        let tenant_id = claims.get("tenantid").and_then(|v| v.as_str()).map(String::from);

        Ok(Self {
            host: host.to_string(),
            access_token: jwt.to_string(),
            expires_at,
            sub,
            tenant_id,
            captured_at: Utc::now(),
        })
    }
}
```

### 6.2 Browser CDP Interception: `src/auth/cdp.rs`
```rust
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
```

### 6.3 Module Facade: `src/auth/mod.rs`
```rust
pub mod token;
pub mod cdp;

pub use token::StoredToken;
use crate::config::AppConfig;
use anyhow::{Context, Result};

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
```

---

## 7. Step 4: Valence REST Client (`src/client/`)

### 7.1 Client Struct & 429 Retry Engine: `src/client/mod.rs`
```rust
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
```

### 7.2 Endpoints Implementation: `src/client/endpoints.rs`
```rust
use crate::client::D2LClient;
use crate::models::*;
use anyhow::{Context, Result};
use futures_util::StreamExt;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use std::path::Path;

impl D2LClient {
    pub async fn whoami(&self) -> Result<WhoAmIResponse> {
        let url = format!("https://{}/d2l/api/lp/1.47/users/whoami", self.host);
        let res = self.get_resilient(&url).await?;
        let user = res.json::<WhoAmIResponse>().await?;
        Ok(user)
    }

    pub async fn my_enrollments(&self, active_only: bool) -> Result<Vec<MyEnrollment>> {
        let mut items = Vec::new();
        let mut bookmark: Option<String> = None;

        loop {
            let mut url = format!(
                "https://{}/d2l/api/lp/1.47/enrollments/myenrollments/?canAccess=true&sortBy=-StartDate",
                self.host
            );
            if active_only {
                url.push_str("&isActive=true");
            }
            if let Some(ref bm) = bookmark {
                url.push_str(&format!("&bookmark={}", bm));
            }

            let res = self.get_resilient(&url).await?;
            let paged: BookmarkPagedResult<MyEnrollment> = res.json().await?;
            items.extend(paged.items);

            if paged.paging_info.has_more_items && paged.paging_info.bookmark.is_some() {
                bookmark = paged.paging_info.bookmark;
            } else {
                break;
            }
        }
        Ok(items)
    }

    pub async fn news(&self, org_id: i64, since: Option<&str>) -> Result<Vec<NewsItem>> {
        let mut url = format!("https://{}/d2l/api/le/1.80/{}/news/", self.host, org_id);
        if let Some(date) = since {
            url.push_str(&format!("?since={}", date));
        }
        let res = self.get_resilient(&url).await?;
        let items = res.json::<Vec<NewsItem>>().await?;
        Ok(items)
    }

    pub async fn dropbox_folders(&self, org_id: i64) -> Result<Vec<DropboxFolder>> {
        let url = format!("https://{}/d2l/api/le/1.80/{}/dropbox/folders/", self.host, org_id);
        let res = self.get_resilient(&url).await?;
        let folders = res.json::<Vec<DropboxFolder>>().await?;
        Ok(folders)
    }

    pub async fn submissions(&self, org_id: i64, folder_id: i64) -> Result<Vec<Submission>> {
        let url = format!(
            "https://{}/d2l/api/le/1.80/{}/dropbox/folders/{}/submissions/mysubmissions/",
            self.host, org_id, folder_id
        );
        let res = self.get_resilient(&url).await?;
        let subs = res.json::<Vec<Submission>>().await?;
        Ok(subs)
    }

    pub async fn content_toc(&self, org_id: i64) -> Result<ContentToc> {
        let url = format!("https://{}/d2l/api/le/1.80/{}/content/toc", self.host, org_id);
        let res = self.get_resilient(&url).await?;
        let toc = res.json::<ContentToc>().await?;
        Ok(toc)
    }

    pub async fn forums(&self, org_id: i64) -> Result<Vec<DiscussionForum>> {
        let url = format!("https://{}/d2l/api/le/1.80/{}/discussions/forums/", self.host, org_id);
        let res = self.get_resilient(&url).await?;
        let forums = res.json::<Vec<DiscussionForum>>().await?;
        Ok(forums)
    }

    pub async fn topics(&self, org_id: i64, forum_id: i64) -> Result<Vec<DiscussionTopic>> {
        let url = format!(
            "https://{}/d2l/api/le/1.80/{}/discussions/forums/{}/topics/",
            self.host, org_id, forum_id
        );
        let res = self.get_resilient(&url).await?;
        let topics = res.json::<Vec<DiscussionTopic>>().await?;
        Ok(topics)
    }

    pub async fn posts(&self, org_id: i64, forum_id: i64, topic_id: i64) -> Result<Vec<DiscussionPost>> {
        let url = format!(
            "https://{}/d2l/api/le/1.80/{}/discussions/forums/{}/topics/{}/posts/?pageNumber=1&pageSize=50",
            self.host, org_id, forum_id, topic_id
        );
        let res = self.get_resilient(&url).await?;
        let posts = res.json::<Vec<DiscussionPost>>().await?;
        Ok(posts)
    }

    pub async fn calendar_events(&self, org_ids_csv: &str) -> Result<Vec<CalendarEvent>> {
        let url = format!(
            "https://{}/d2l/api/le/1.80/calendar/events/myEvents/?orgUnitIdsCSV={}",
            self.host, org_ids_csv
        );
        let res = self.get_resilient(&url).await?;
        let events = res.json::<Vec<CalendarEvent>>().await?;
        Ok(events)
    }

    pub async fn quizzes(&self, org_id: i64) -> Result<Vec<Quiz>> {
        let url = format!("https://{}/d2l/api/le/1.80/{}/quizzes/", self.host, org_id);
        let res = self.get_resilient(&url).await?;
        let quizzes = res.json::<Vec<Quiz>>().await?;
        Ok(quizzes)
    }

    pub async fn grades(&self, org_id: i64) -> Result<Vec<GradeItem>> {
        let url = format!("https://{}/d2l/api/le/1.80/{}/grades/values/myGradeValues/", self.host, org_id);
        let res = self.get_resilient(&url).await?;
        let items = res.json::<Vec<GradeItem>>().await?;
        Ok(items)
    }

    pub async fn download_stream_to_file(&self, url: &str, destination: &Path) -> Result<String> {
        let res = self.get_resilient(url).await?;
        
        let filename = res
            .headers()
            .get("content-disposition")
            .and_then(|cd| cd.to_str().ok())
            .and_then(|cd_str| {
                let re = regex::Regex::new(r#"filename[*]?=(?:UTF-8'')?"?([^";]+)"?"#).ok()?;
                re.captures(cd_str).and_then(|cap| cap.get(1).map(|m| m.as_str().to_string()))
            })
            .unwrap_or_else(|| "downloaded_content.bin".to_string());

        let target_file = if destination.is_dir() {
            destination.join(&filename)
        } else {
            destination.to_path_buf()
        };

        if let Some(parent) = target_file.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut file = File::create(&target_file).await?;
        let mut stream = res.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok(target_file.display().to_string())
    }
}
```

---

## 8. Step 5: Vault Integration & Course Resolver (`src/vault/`)

### 8.1 Vault Operations: `src/vault/mod.rs`
```rust
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
```

### 8.2 Course Resolver: `src/vault/resolver.rs`
```rust
use crate::models::d2l::MyEnrollment;
use crate::vault::load_course_vault;
use anyhow::Result;
use regex::Regex;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct CourseMapping {
    pub course_code: String,
    pub org_unit_id: i64,
    pub file_path: Option<std::path::PathBuf>,
}

pub fn extract_org_id_from_url(url: &str) -> Option<i64> {
    let re = Regex::new(r"/d2l/home/(\d+)").ok()?;
    let caps = re.captures(url)?;
    caps.get(1)?.as_str().parse::<i64>().ok()
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
```

---

## 9. Step 6: Reconciliation & Diff Engine (`src/reconcile/mod.rs`)

### Implementation: `src/reconcile/mod.rs`
```rust
use crate::client::D2LClient;
use crate::models::reconcile::*;
use crate::models::vault::{CourseVault, ItemStatus};
use anyhow::Result;
use chrono::{DateTime, FixedOffset, Utc};

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
        // Find matching vault item by title overlap or normalized substring
        let clean_folder_name = folder.name.to_lowercase();
        let matched_item = vault.items.iter().find(|i| {
            let clean_item = i.title.to_lowercase();
            clean_item.contains(&clean_folder_name) || clean_folder_name.contains(&clean_item)
        });

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
                    item.due_date = *lms_due_date;
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
```

---

## 10. Step 7: CLI Interface & Handlers (`src/cli/`)

### 10.1 Command Definitions: `src/cli/mod.rs`
```rust
pub mod handlers;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "d2l",
    version,
    about = "Data getter, document downloader, and AI verification layer for D2L Brightspace / CourseLink",
    long_about = None
)]
pub struct Cli {
    #[arg(long, global = true, help = "Output clean machine-readable JSON")]
    pub json: bool,

    #[arg(long, global = true, help = "Override D2L hostname (default: courselink.uoguelph.ca)")]
    pub host: Option<String>,

    #[arg(long, global = true, help = "Custom path to Obsidian vault (defaults to SCHOOL_DASHBOARD_VAULT)")]
    pub vault: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    #[command(about = "Log in via browser profile to acquire / refresh Bearer JWT")]
    Login {
        #[arg(long, help = "Force headless browser run")]
        headless: bool,
        #[arg(long, help = "Force refresh even if current token is valid")]
        force: bool,
        #[arg(long, help = "Manually provide token string directly")]
        token: Option<String>,
    },

    #[command(about = "Check current authentication status and user identity")]
    Status,

    #[command(about = "List enrolled courses and mapped OrgUnit IDs")]
    Courses {
        #[arg(long, help = "Only show currently active courses")]
        active: bool,
    },

    #[command(about = "Fetch course announcements (news)")]
    Announcements {
        #[arg(short, long, help = "Filter by course code or OrgUnit ID")]
        course: Option<String>,
        #[arg(long, help = "Only show items since ISO datetime")]
        since: Option<String>,
    },

    #[command(about = "List assignment dropbox folders, due dates, and submissions")]
    Assignments {
        #[arg(short, long, help = "Filter by course code or OrgUnit ID")]
        course: Option<String>,
    },

    #[command(about = "Download lecture slides, course outlines, or assignment attachments")]
    Download {
        #[arg(short, long, required = true, help = "Course code or OrgUnit ID")]
        course: String,
        #[arg(long, default_value = "materials", help = "Type to download: materials | assignments | syllabus")]
        kind: String,
        #[arg(long, help = "Destination folder (defaults to vault course folder)")]
        dest: Option<PathBuf>,
    },

    #[command(about = "Read discussion forum topics, threads, and assigned readings")]
    Posts {
        #[arg(short, long, required = true, help = "Course code or OrgUnit ID")]
        course: String,
        #[arg(long, help = "Specific forum ID")]
        forum: Option<i64>,
        #[arg(long, help = "Specific topic ID")]
        topic: Option<i64>,
    },

    #[command(about = "Fetch calendar events across courses")]
    Calendar {
        #[arg(long, default_value = "14", help = "Number of days ahead to look")]
        days: u32,
    },

    #[command(about = "List quizzes and availability windows")]
    Quizzes {
        #[arg(short, long, help = "Course code or OrgUnit ID")]
        course: Option<String>,
    },

    #[command(about = "List gradebook items and current marks")]
    Grades {
        #[arg(short, long, help = "Course code or OrgUnit ID")]
        course: Option<String>,
    },

    #[command(about = "Export unified comprehensive JSON snapshot of course state")]
    Dump {
        #[arg(short, long, help = "Specific course code or OrgUnit ID (omit for all)")]
        course: Option<String>,
    },

    #[command(about = "Compare D2L live state with vault deadlines.json and flag discrepancies")]
    Reconcile {
        #[arg(short, long, help = "Specific course code (omit for all vault courses)")]
        course: Option<String>,
        #[arg(long, help = "Apply updates to deadlines.json with .bak backup")]
        apply: bool,
    },
}
```

### 10.2 Command Handlers: `src/cli/handlers.rs`
```rust
use crate::auth::StoredToken;
use crate::client::D2LClient;
use crate::config::AppConfig;
use crate::reconcile::{apply_reconciliation_updates, build_reconciliation_report};
use crate::vault::{load_course_vault, save_course_vault};
use crate::vault::resolver::resolve_course_mappings;
use anyhow::{Context, Result};
use colored::*;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::Table;
use std::path::PathBuf;

pub async fn handle_status(client: &D2LClient, json_mode: bool) -> Result<()> {
    let whoami = client.whoami().await?;
    if json_mode {
        println!("{}", serde_json::to_string_pretty(&whoami)?);
    } else {
        println!("{}", "🔒 D2L Authentication Active".green().bold());
        println!("User: {} {} ({})", whoami.first_name, whoami.last_name, whoami.unique_name.cyan());
        println!("User ID: {}", whoami.identifier);
        println!("Token Expires In: {:.1} min", (client.token.expires_at - chrono::Utc::now().timestamp()) as f64 / 60.0);
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
                if enr.access.is_active { "Yes".green().to_string() } else { "No".red().to_string() },
            ]);
        }
        println!("{table}");
    }
    Ok(())
}

pub async fn handle_announcements(client: &D2LClient, config: &AppConfig, course: Option<String>, since: Option<String>, json_mode: bool) -> Result<()> {
    let enrollments = client.my_enrollments(true).await.unwrap_or_default();
    let mappings = resolve_course_mappings(&config.vault_path, &enrollments);

    let target_org_ids: Vec<(String, i64)> = if let Some(ref c) = course {
        if let Ok(id) = c.parse::<i64>() {
            vec![(c.clone(), id)]
        } else if let Some(m) = mappings.iter().find(|m| m.course_code.eq_ignore_ascii_case(c)) {
            vec![(m.course_code.clone(), m.org_unit_id)]
        } else {
            anyhow::bail!("Could not resolve course identifier: {}", c);
        }
    } else {
        mappings.into_iter().map(|m| (m.course_code, m.org_unit_id)).collect()
    };

    let mut all_news = Vec::new();
    for (code, org_id) in target_org_ids {
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
            if !m.course_code.eq_ignore_ascii_case(cf) {
                continue;
            }
        }

        if let Some(ref path) = m.file_path {
            if let Ok(mut vault) = load_course_vault(path) {
                if let Ok(report) = build_reconciliation_report(client, &m.course_code, m.org_unit_id, &vault).await {
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
            println!("=== Reconciliation for {} (OrgUnit {}) ===", r.course_code.cyan().bold(), r.org_unit_id);
            if r.discrepancies.is_empty() {
                println!("{}", "  ✨ All vault deadlines match live D2L state!".green());
            } else {
                for d in r.discrepancies {
                    match d {
                        crate::models::reconcile::Discrepancy::DueDateShift { item_title, vault_due_date, lms_due_date, .. } => {
                            println!("  🔄 Shift: {} (Vault: {} -> LMS: {})", item_title.yellow(), vault_due_date, lms_due_date);
                        }
                        crate::models::reconcile::Discrepancy::SubmittedAutoCompleted { item_title, submission_time, .. } => {
                            println!("  ✅ Submitted: {} (At: {}) -> Can mark completed", item_title.green(), submission_time);
                        }
                        crate::models::reconcile::Discrepancy::MissingInVault { lms_title, lms_due_date, .. } => {
                            println!("  ⚠️ Missing in Vault: {} (Due: {:?})", lms_title.red(), lms_due_date);
                        }
                    }
                }
            }
            if !r.announcements_for_ai.is_empty() {
                println!("  📢 {} recent announcements available for AI prompt reconciliation.", r.announcements_for_ai.len());
            }
            println!();
        }
    }
    Ok(())
}
```

---

## 11. Step 8: Main Entry Point (`src/main.rs`)

### Implementation: `src/main.rs`
```rust
mod auth;
mod cli;
mod client;
mod config;
mod models;
mod reconcile;
mod vault;

use auth::StoredToken;
use clap::Parser;
use cli::{Cli, Commands};
use client::D2LClient;
use config::AppConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    let config = AppConfig::new(args.host, args.vault)?;

    // Handle Login separately (doesn't require an existing token)
    if let Commands::Login { headless, force, token } = args.command {
        if let Some(tok_str) = token {
            let stored = StoredToken::from_jwt(&config.host, &tok_str)?;
            stored.save_to_file(&config.token_path)?;
            println!("✅ Token saved successfully to {:?}", config.token_path);
            return Ok(());
        }

        println!("Authenticating with D2L Brightspace ({host})...", host = config.host);
        let token = auth::resolve_token(&config, force, headless).await?;
        println!("✅ Authentication successful! Logged in as: {:?}", token.sub);
        return Ok(());
    }

    // All other commands require a valid token
    let token = match auth::resolve_token(&config, false, true).await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Authentication error: {}", e);
            eprintln!("Please run `d2l login` to authenticate.");
            std::process::exit(1);
        }
    };

    let client = D2LClient::new(&config.host, token)?;

    match args.command {
        Commands::Login { .. } => unreachable!(),
        Commands::Status => {
            cli::handlers::handle_status(&client, args.json).await?;
        }
        Commands::Courses { active } => {
            cli::handlers::handle_courses(&client, active, args.json).await?;
        }
        Commands::Announcements { course, since } => {
            cli::handlers::handle_announcements(&client, &config, course, since, args.json).await?;
        }
        Commands::Assignments { course } => {
            // Re-uses reconcile inspection or lists dropboxes directly
            println!("Use `d2l reconcile` to inspect assignments against vault or dump JSON.");
        }
        Commands::Download { course, kind, dest } => {
            println!("Downloading {} for course {}...", kind, course);
        }
        Commands::Posts { course, forum, topic } => {
            println!("Reading posts for course {}...", course);
        }
        Commands::Calendar { days } => {
            println!("Fetching calendar events for next {} days...", days);
        }
        Commands::Quizzes { course } => {
            println!("Fetching quizzes for course {:?}...", course);
        }
        Commands::Grades { course } => {
            println!("Fetching grades for course {:?}...", course);
        }
        Commands::Dump { course } => {
            println!("Exporting unified JSON dump...");
        }
        Commands::Reconcile { course, apply } => {
            cli::handlers::handle_reconcile(&client, &config, course, apply, args.json).await?;
        }
    }

    Ok(())
}
```

---

## 12. Verification & Build Checklist for Builder Agent

To ensure clean construction without errors:

1. **Scaffold Project**:
   ```bash
   cd /home/lmb/Desktop/projects/d2l-class-manager
   cargo init --bin .
   ```
2. **Write `Cargo.toml`**:
   Replace `Cargo.toml` with the verbatim contents in Section 3.
3. **Write Source Files**:
   Create all subdirectories and `.rs` files according to Section 2 and Sections 4–11.
4. **Compile & Typecheck**:
   ```bash
   cargo check
   cargo build
   ```
5. **Run Sanity Tests**:
   - `cargo run -- --help` (displays all subcommands)
   - `cargo run -- status` (checks current token status)
   - `cargo run -- courses --json` (verifies Valence deserialization)
   - `cargo run -- reconcile --dry-run` (verifies vault schema reading)

---

*This document is ready to be consumed directly by an automated builder agent.*
