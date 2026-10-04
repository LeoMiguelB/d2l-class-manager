use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredToken {
    pub host: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub cookies: Option<String>,
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
        let clean_jwt = jwt.trim().strip_prefix("Bearer ").unwrap_or(jwt.trim()).trim();
        let parts: Vec<&str> = clean_jwt.split('.').collect();
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
            access_token: clean_jwt.to_string(),
            cookies: None,
            expires_at,
            sub,
            tenant_id,
            captured_at: Utc::now(),
        })
    }

    pub fn from_cookies(host: &str, cookies: &str, username: Option<&str>) -> Self {
        let now = Utc::now();
        // Session cookies typically remain valid for 2 hours
        let expires_at = now.timestamp() + 2 * 3600;
        Self {
            host: host.to_string(),
            access_token: String::new(),
            cookies: Some(cookies.to_string()),
            expires_at,
            sub: username.map(|s| s.to_string()),
            tenant_id: None,
            captured_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn make_test_jwt(sub: &str, exp: i64) -> String {
        use base64::Engine;
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"alg":"none","typ":"JWT"}"#);
        let payload_json = serde_json::json!({
            "sub": sub,
            "exp": exp,
            "tenantid": "uoguelph"
        });
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload_json.to_string());
        format!("{}.{}.fake_sig", header, payload)
    }

    #[test]
    fn test_from_jwt_valid() {
        let now = Utc::now().timestamp();
        let jwt = make_test_jwt("test_student", now + 3600);
        let token = StoredToken::from_jwt("courselink.uoguelph.ca", &jwt).unwrap();

        assert_eq!(token.host, "courselink.uoguelph.ca");
        assert_eq!(token.access_token, jwt);
        assert_eq!(token.sub.as_deref(), Some("test_student"));
        assert_eq!(token.tenant_id.as_deref(), Some("uoguelph"));
        assert!(token.is_valid());
    }

    #[test]
    fn test_from_jwt_with_bearer_prefix() {
        let now = Utc::now().timestamp();
        let raw_jwt = make_test_jwt("student_bearer", now + 3600);
        let bearer_input = format!("Bearer  {}  ", raw_jwt);
        let token = StoredToken::from_jwt("courselink.uoguelph.ca", &bearer_input).unwrap();

        assert_eq!(token.access_token, raw_jwt);
        assert_eq!(token.sub.as_deref(), Some("student_bearer"));
    }

    #[test]
    fn test_token_expiry() {
        let now = Utc::now().timestamp();
        // Expired 10 seconds ago
        let expired_jwt = make_test_jwt("expired_student", now - 10);
        let token = StoredToken::from_jwt("courselink.uoguelph.ca", &expired_jwt).unwrap();
        assert!(!token.is_valid());

        // Expires in 30 seconds (within 60s safety buffer)
        let almost_expired_jwt = make_test_jwt("student_buffer", now + 30);
        let token_buf = StoredToken::from_jwt("courselink.uoguelph.ca", &almost_expired_jwt).unwrap();
        assert!(!token_buf.is_valid());
    }

    #[test]
    fn test_token_file_save_and_load() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path();

        let now = Utc::now().timestamp();
        let jwt = make_test_jwt("file_student", now + 3600);
        let token = StoredToken::from_jwt("courselink.uoguelph.ca", &jwt).unwrap();

        token.save_to_file(path).unwrap();
        let loaded = StoredToken::load_from_file(path).unwrap().expect("Token should be loaded");

        assert_eq!(loaded.host, token.host);
        assert_eq!(loaded.access_token, token.access_token);
        assert_eq!(loaded.sub, token.sub);
    }
}
