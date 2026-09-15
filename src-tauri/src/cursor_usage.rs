use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::Value;

use crate::dirs_home;

const USAGE_SUMMARY_URL: &str = "https://cursor.com/api/usage-summary";
const USER_AGENT: &str = "MonoCode";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const TOKEN_EXPIRY_BUFFER_SECS: i64 = 60;
const AUTH_TOKEN_KEY: &str = "cursorAuth/accessToken";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorUsageFetch {
    pub status: String,
    pub http_status: Option<u16>,
    pub body: Option<String>,
    pub error: Option<String>,
}

/// Fetch Cursor plan usage via the signed-in Cursor.app session.
/// The access token never leaves the host process.
#[tauri::command]
pub async fn fetch_cursor_usage() -> Result<CursorUsageFetch, String> {
    tauri::async_runtime::spawn_blocking(fetch_cursor_usage_sync)
        .await
        .map_err(|e| e.to_string())?
}

fn fetch_cursor_usage_sync() -> Result<CursorUsageFetch, String> {
    let Some(token) = read_cursor_access_token() else {
        return Ok(usage_result(
            "unavailable",
            None,
            None,
            Some("Cursor not signed in".into()),
        ));
    };
    if !jwt_is_usable(&token, now_secs()) {
        return Ok(usage_result(
            "unavailable",
            None,
            None,
            Some("Cursor sign-in expired".into()),
        ));
    }
    let Some(cookie) = cursor_cookie_header(&token) else {
        return Ok(usage_result(
            "unavailable",
            None,
            None,
            Some("Cursor sign-in is invalid".into()),
        ));
    };
    Ok(fetch_usage_with_cookie(&cookie))
}

fn usage_result(
    status: &str,
    http_status: Option<u16>,
    body: Option<String>,
    error: Option<String>,
) -> CursorUsageFetch {
    CursorUsageFetch {
        status: status.into(),
        http_status,
        body,
        error,
    }
}

fn fetch_usage_with_cookie(cookie: &str) -> CursorUsageFetch {
    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
    let result = agent
        .get(USAGE_SUMMARY_URL)
        .set("Accept", "application/json")
        .set("Cookie", cookie)
        .set("User-Agent", USER_AGENT)
        .call();

    match result {
        Ok(response) => {
            let http_status = response.status();
            let body = response.into_string().unwrap_or_default();
            if (200..300).contains(&http_status) {
                usage_result("ok", Some(http_status), Some(body), None)
            } else {
                usage_error(http_status)
            }
        }
        Err(ureq::Error::Status(status, response)) => {
            let _ = response.into_string();
            usage_error(status)
        }
        Err(error) => usage_result(
            "error",
            None,
            None,
            Some(format!("Cursor usage request failed: {error}")),
        ),
    }
}

fn usage_error(status: u16) -> CursorUsageFetch {
    let (kind, message) = if status == 401 || status == 403 {
        ("unavailable", "Cursor not signed in".into())
    } else {
        ("error", format!("Cursor usage request failed ({status})"))
    };
    usage_result(kind, Some(status), None, Some(message))
}

fn read_cursor_access_token() -> Option<String> {
    let path = cursor_state_db_path()?;
    if !path.is_file() {
        return None;
    }
    let token = read_item_table_value(&path, AUTH_TOKEN_KEY).ok()??;
    let trimmed = token.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn cursor_state_db_path() -> Option<PathBuf> {
    let home = dirs_home()?;
    Some(cursor_state_db_path_for(&home))
}

pub(crate) fn cursor_state_db_path_for(home: &str) -> PathBuf {
    let home = Path::new(home);
    #[cfg(target_os = "macos")]
    {
        home.join("Library/Application Support/Cursor/User/globalStorage/state.vscdb")
    }
    #[cfg(target_os = "windows")]
    {
        home.join("AppData/Roaming/Cursor/User/globalStorage/state.vscdb")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        config.join("Cursor/User/globalStorage/state.vscdb")
    }
}

fn read_item_table_value(path: &Path, key: &str) -> Result<Option<String>, rusqlite::Error> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_millis(250))?;
    let mut statement = connection.prepare("SELECT value FROM ItemTable WHERE key = ?1 LIMIT 1")?;
    let mut rows = statement.query(rusqlite::params![key])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(decode_sqlite_text(row.get_ref(0)?))
}

fn decode_sqlite_text(value: rusqlite::types::ValueRef<'_>) -> Option<String> {
    match value {
        rusqlite::types::ValueRef::Text(bytes) => String::from_utf8(bytes.to_vec())
            .ok()
            .or_else(|| decode_utf16le(bytes)),
        rusqlite::types::ValueRef::Blob(bytes) => {
            decode_utf16le(bytes).or_else(|| String::from_utf8(bytes.to_vec()).ok())
        }
        rusqlite::types::ValueRef::Null => None,
        _ => None,
    }
}

fn decode_utf16le(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 2 || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let (pairs, _) = bytes.as_chunks::<2>();
    let ascii_utf16le = pairs
        .iter()
        .all(|pair| (1..128).contains(&pair[0]) && pair[1] == 0);
    if !ascii_utf16le {
        return None;
    }
    String::from_utf16(
        &pairs
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect::<Vec<_>>(),
    )
    .ok()
}

pub(crate) fn jwt_is_usable(token: &str, now_secs: i64) -> bool {
    let Some(payload) = jwt_payload(token) else {
        return false;
    };
    let Some(exp) = payload.get("exp").and_then(Value::as_i64) else {
        return false;
    };
    exp - now_secs > TOKEN_EXPIRY_BUFFER_SECS
}

pub(crate) fn cursor_cookie_header(token: &str) -> Option<String> {
    let user_id = jwt_user_id(token)?;
    Some(format!("WorkosCursorSessionToken={user_id}%3A%3A{token}"))
}

pub(crate) fn jwt_user_id(token: &str) -> Option<String> {
    let payload = jwt_payload(token)?;
    let subject = payload.get("sub")?.as_str()?.trim();
    if subject.is_empty() {
        return None;
    }
    let user_id = subject.rsplit('|').next().unwrap_or(subject).trim();
    if user_id.is_empty() {
        return None;
    }
    if !user_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return None;
    }
    Some(user_id.to_string())
}

fn jwt_payload(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    if payload.is_empty() || parts.next().is_none() {
        return None;
    }
    let mut encoded = payload.replace('-', "+").replace('_', "/");
    match encoded.len() % 4 {
        2 => encoded.push_str("=="),
        3 => encoded.push('='),
        0 => {}
        _ => return None,
    }
    let bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        encoded.as_bytes(),
    )
    .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn test_jwt(sub: &str, exp: i64) -> String {
        let header = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            br#"{"alg":"none"}"#,
        );
        let payload = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            format!(r#"{{"sub":"{sub}","exp":{exp}}}"#).as_bytes(),
        );
        format!("{header}.{payload}.sig")
    }

    #[test]
    fn jwt_user_id_takes_the_last_subject_segment() {
        let token = test_jwt("auth0|user_abc-1", 2_000_000_000);
        assert_eq!(jwt_user_id(&token).as_deref(), Some("user_abc-1"));
    }

    #[test]
    fn jwt_user_id_rejects_invalid_characters() {
        let token = test_jwt("auth0|user/abc", 2_000_000_000);
        assert_eq!(jwt_user_id(&token), None);
    }

    #[test]
    fn jwt_is_usable_requires_a_future_expiry() {
        let token = test_jwt("user_1", 1_000_061);
        assert!(jwt_is_usable(&token, 1_000_000));
        assert!(!jwt_is_usable(&token, 1_000_002));
    }

    #[test]
    fn cookie_header_uses_the_cursor_session_shape() {
        let token = test_jwt("auth0|user_1", 2_000_000_000);
        let expected = format!("WorkosCursorSessionToken=user_1%3A%3A{token}");
        assert_eq!(
            cursor_cookie_header(&token).as_deref(),
            Some(expected.as_str())
        );
    }

    #[test]
    fn macos_state_db_lives_under_application_support() {
        let path = cursor_state_db_path_for("/Users/ada");
        #[cfg(target_os = "macos")]
        assert_eq!(
            path,
            PathBuf::from(
                "/Users/ada/Library/Application Support/Cursor/User/globalStorage/state.vscdb"
            )
        );
        #[cfg(not(target_os = "macos"))]
        let _ = path;
    }

    #[test]
    fn reads_item_table_text_and_utf16le_blobs() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute(
                "CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                params!["cursorAuth/accessToken", "plain-token"],
            )
            .unwrap();
        let utf16: Vec<u8> = "utf16-token"
            .encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        connection
            .execute(
                "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                params!["other", utf16],
            )
            .unwrap();

        let mut statement = connection
            .prepare("SELECT value FROM ItemTable WHERE key = ?1")
            .unwrap();
        let text = statement
            .query_row(params!["cursorAuth/accessToken"], |row| {
                Ok(decode_sqlite_text(row.get_ref(0)?))
            })
            .unwrap();
        assert_eq!(text.as_deref(), Some("plain-token"));
        let blob = statement
            .query_row(params!["other"], |row| {
                Ok(decode_sqlite_text(row.get_ref(0)?))
            })
            .unwrap();
        assert_eq!(blob.as_deref(), Some("utf16-token"));
    }

    #[test]
    fn even_length_utf8_jwt_blob_falls_back_to_utf8() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute(
                "CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB)",
                [],
            )
            .unwrap();
        // Even-length ASCII JWT: naive UTF-16LE decoding would mojibake it.
        let token = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1In0.sig";
        assert_eq!(token.len() % 2, 0);
        assert_eq!(decode_utf16le(token.as_bytes()), None);
        connection
            .execute(
                "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                params!["cursorAuth/accessToken", token.as_bytes()],
            )
            .unwrap();
        let decoded = connection
            .query_row(
                "SELECT value FROM ItemTable WHERE key = ?1",
                params!["cursorAuth/accessToken"],
                |row| Ok(decode_sqlite_text(row.get_ref(0)?)),
            )
            .unwrap();
        assert_eq!(decoded.as_deref(), Some(token));
    }
}
