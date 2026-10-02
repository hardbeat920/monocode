//! Token usage read from the Claude Code and Codex session logs on disk.
//!
//! Each provider CLI writes one JSONL transcript per session into its config
//! directory. This module walks the directory for one account profile, keeps
//! the per-request token counts newer than a cutoff, and folds them into
//! 15-minute buckets per model and working directory. Pricing and calendar-day
//! grouping happen in the webview, which knows the user's time zone.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

/// Bucket width. Fine enough that every time zone offset (including the
/// 30- and 45-minute ones) lands a bucket on the right local day.
const SLOT_SECONDS: i64 = 15 * 60;
/// How deep to look below the log root. Claude nests subagent transcripts
/// under `projects/<project>/<session>/subagents/`; Codex uses
/// `sessions/YYYY/MM/DD/`.
const MAX_DEPTH: usize = 5;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRow {
    /// Start of the 15-minute bucket, in Unix seconds (UTC).
    pub slot: i64,
    pub model: String,
    /// Working directory the session ran in. Empty when the log has none.
    pub project: String,
    /// Input tokens billed at the full rate (cache reads and writes excluded).
    pub input: u64,
    pub cache_read: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub output: u64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    pub rows: Vec<UsageRow>,
    /// False when the account has no log directory yet.
    pub found: bool,
    pub files_scanned: usize,
}

#[tauri::command]
pub async fn provider_usage_report(
    app: AppHandle,
    provider: String,
    account_id: Option<String>,
    since_ms: i64,
) -> Result<UsageReport, String> {
    let config_dir = account_config_dir(&app, &provider, account_id.as_deref())?;
    tauri::async_runtime::spawn_blocking(move || {
        let since = since_ms.div_euclid(1000);
        Ok(match provider.as_str() {
            "claude" => scan(&[config_dir.join("projects")], since, parse_claude_file),
            // Codex moves sessions the user archives out of `sessions`.
            _ => scan(
                &[
                    config_dir.join("sessions"),
                    config_dir.join("archived_sessions"),
                ],
                since,
                parse_codex_file,
            ),
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The directory a provider CLI uses for this account, without creating it.
fn account_config_dir(
    app: &AppHandle,
    provider: &str,
    account_id: Option<&str>,
) -> Result<PathBuf, String> {
    if provider != "claude" && provider != "codex" {
        return Err("Usage is only available for Claude Code and Codex".into());
    }
    if let Some(id) = account_id.filter(|id| *id != crate::harness::DEFAULT_PROVIDER_ACCOUNT_ID) {
        return crate::harness::provider_account_path(app, provider, id);
    }
    let (env, dir) = if provider == "claude" {
        ("CLAUDE_CONFIG_DIR", ".claude")
    } else {
        ("CODEX_HOME", ".codex")
    };
    match std::env::var_os(env).filter(|value| !value.is_empty()) {
        Some(path) => Ok(PathBuf::from(path)),
        None => {
            Ok(PathBuf::from(crate::dirs_home().ok_or("Home directory is unavailable")?).join(dir))
        }
    }
}

type FileParser = fn(&Path, i64, &mut Totals, &mut Requests);
type Totals = HashMap<(i64, String, String), UsageRow>;

/// A request seen in the logs, keyed by its ids, before it is bucketed.
struct Request {
    timestamp: i64,
    model: String,
    project: String,
    usage: UsageRow,
}
type Requests = HashMap<String, Request>;

fn scan(roots: &[PathBuf], since: i64, parse: FileParser) -> UsageReport {
    let roots: Vec<&PathBuf> = roots.iter().filter(|root| root.is_dir()).collect();
    if roots.is_empty() {
        return UsageReport::default();
    }
    let mut files = Vec::new();
    let mut names = HashMap::new();
    for (index, root) in roots.iter().enumerate() {
        let mut found = Vec::new();
        collect_jsonl(root, since, 0, &mut found);
        for (modified, path) in found {
            // The same session under two roots (moved while we scan) counts once.
            let name = log_name(&path);
            if *names.entry(name).or_insert(index) == index {
                files.push((modified, path));
            }
        }
    }
    // Oldest first, so a request logged again by a resumed session is counted
    // once, under the transcript that first recorded it.
    files.sort_by_key(|(modified, _)| *modified);

    let mut totals = Totals::new();
    let mut requests = Requests::new();
    for (_, path) in &files {
        parse(path, since, &mut totals, &mut requests);
    }
    for request in requests.into_values() {
        add(
            &mut totals,
            request.timestamp,
            &request.model,
            &request.project,
            &request.usage,
        );
    }
    let mut rows: Vec<UsageRow> = totals.into_values().collect();
    rows.sort_by(|a, b| (a.slot, &a.model, &a.project).cmp(&(b.slot, &b.model, &b.project)));
    UsageReport {
        rows,
        found: true,
        files_scanned: files.len(),
    }
}

fn collect_jsonl(dir: &Path, since: i64, depth: usize, out: &mut Vec<(i64, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            if depth < MAX_DEPTH {
                collect_jsonl(&path, since, depth + 1, out);
            }
            continue;
        }
        if !file_type.is_file() || !is_log(&path) {
            continue;
        }
        // A transcript last written before the cutoff holds nothing newer.
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed: Duration| elapsed.as_secs() as i64)
            .unwrap_or(i64::MAX);
        if modified >= since {
            out.push((modified, path));
        }
    }
}

/// A JSONL transcript, plain or zstd-compressed as Codex can store them.
fn is_log(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"))
}

fn log_name(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    name.strip_suffix(".zst").unwrap_or(name).to_string()
}

fn add(totals: &mut Totals, timestamp: i64, model: &str, project: &str, usage: &UsageRow) {
    let slot = timestamp.div_euclid(SLOT_SECONDS) * SLOT_SECONDS;
    let row = totals
        .entry((slot, model.to_string(), project.to_string()))
        .or_insert_with(|| UsageRow {
            slot,
            model: model.to_string(),
            project: project.to_string(),
            ..UsageRow::default()
        });
    row.input += usage.input;
    row.cache_read += usage.cache_read;
    row.cache_write_5m += usage.cache_write_5m;
    row.cache_write_1h += usage.cache_write_1h;
    row.output += usage.output;
}

/// Lines of a transcript, decompressed on the fly for `.zst` files. Reading
/// stops at the first damaged line or frame.
fn lines(path: &Path) -> Box<dyn Iterator<Item = String>> {
    let Ok(file) = std::fs::File::open(path) else {
        return Box::new(std::iter::empty());
    };
    let file = BufReader::new(file);
    if path.extension().and_then(|ext| ext.to_str()) == Some("zst") {
        let frames = ZstdFrames {
            source: Some(file),
            decoder: None,
        };
        return Box::new(BufReader::new(frames).lines().map_while(Result::ok));
    }
    Box::new(file.lines().map_while(Result::ok))
}

/// Streams every frame of a zstd file in turn, since a file appended to over
/// time can hold several.
struct ZstdFrames<R: BufRead> {
    source: Option<R>,
    decoder: Option<ruzstd::decoding::StreamingDecoder<R, ruzstd::decoding::FrameDecoder>>,
}

impl<R: BufRead> Read for ZstdFrames<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if let Some(decoder) = &mut self.decoder {
                match decoder.read(buf) {
                    Ok(0) if !buf.is_empty() => {
                        // Frame finished; carry on with whatever follows it.
                        self.source = self.decoder.take().map(|decoder| decoder.into_inner());
                    }
                    result => return result,
                }
            }
            let Some(mut source) = self.source.take() else {
                return Ok(0);
            };
            if source.fill_buf()?.is_empty() {
                return Ok(0);
            }
            let decoder = ruzstd::decoding::StreamingDecoder::new(source)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            self.decoder = Some(decoder);
        }
    }
}

fn count(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// Claude Code writes one line per content block of an assistant message, each
/// repeating the message's usage, so requests are keyed by message and request
/// id and counted once. An early line can carry a partial snapshot (output
/// still streaming), so each count keeps the largest value logged for it.
fn parse_claude_file(path: &Path, since: i64, _totals: &mut Totals, requests: &mut Requests) {
    for line in lines(path) {
        if !line.contains("\"usage\"") || !line.contains("\"assistant\"") {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some((timestamp, model, project, usage, key)) = claude_usage(&entry) {
            if timestamp < since {
                continue;
            }
            match requests.get_mut(&key) {
                Some(request) => {
                    let kept = &mut request.usage;
                    kept.input = kept.input.max(usage.input);
                    kept.cache_read = kept.cache_read.max(usage.cache_read);
                    kept.cache_write_5m = kept.cache_write_5m.max(usage.cache_write_5m);
                    kept.cache_write_1h = kept.cache_write_1h.max(usage.cache_write_1h);
                    kept.output = kept.output.max(usage.output);
                }
                None => {
                    requests.insert(
                        key,
                        Request {
                            timestamp,
                            model,
                            project,
                            usage,
                        },
                    );
                }
            }
        }
    }
}

fn claude_usage(entry: &Value) -> Option<(i64, String, String, UsageRow, String)> {
    if entry.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let message = entry.get("message")?;
    let model = message.get("model")?.as_str()?;
    // Locally generated notices ("<synthetic>") were never billed.
    if model.starts_with('<') {
        return None;
    }
    let usage = message.get("usage")?;
    let timestamp = parse_timestamp(entry.get("timestamp")?.as_str()?)?;
    let cache_write = count(usage, "cache_creation_input_tokens");
    let (write_5m, write_1h) = match usage.get("cache_creation") {
        Some(split) => (
            count(split, "ephemeral_5m_input_tokens"),
            count(split, "ephemeral_1h_input_tokens"),
        ),
        None => (cache_write, 0),
    };
    let row = UsageRow {
        input: count(usage, "input_tokens"),
        cache_read: count(usage, "cache_read_input_tokens"),
        // Older logs report only the total; treat it all as 5-minute writes.
        cache_write_5m: if write_5m + write_1h == 0 {
            cache_write
        } else {
            write_5m
        },
        cache_write_1h: write_1h,
        output: count(usage, "output_tokens"),
        ..UsageRow::default()
    };
    let message_id = message.get("id").and_then(Value::as_str).unwrap_or("");
    let request_id = entry.get("requestId").and_then(Value::as_str).unwrap_or("");
    let key = if message_id.is_empty() && request_id.is_empty() {
        entry
            .get("uuid")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    } else {
        format!("{message_id}:{request_id}")
    };
    let project = entry
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Some((timestamp, model.to_string(), project, row, key))
}

/// Codex logs a running total after each model response. `last_token_usage`
/// is that response alone; a repeated total means the same response was
/// reported twice. A resumed or forked session replays earlier lines into a
/// new rollout, so responses are also keyed across files and counted once.
fn parse_codex_file(path: &Path, since: i64, _totals: &mut Totals, requests: &mut Requests) {
    let mut model = String::new();
    let mut project = String::new();
    let mut last_total: Option<u64> = None;
    for line in lines(path) {
        let wanted = line.contains("token_count")
            || line.contains("turn_context")
            || line.contains("session_meta");
        if !wanted {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(payload) = entry.get("payload") else {
            continue;
        };
        match entry.get("type").and_then(Value::as_str) {
            Some("session_meta") | Some("turn_context") => {
                if let Some(cwd) = payload.get("cwd").and_then(Value::as_str) {
                    project = cwd.to_string();
                }
                if let Some(name) = payload.get("model").and_then(Value::as_str) {
                    model = name.to_string();
                }
            }
            Some("event_msg")
                if payload.get("type").and_then(Value::as_str) == Some("token_count") =>
            {
                let Some(info) = payload.get("info").filter(|info| !info.is_null()) else {
                    continue;
                };
                let total = info
                    .get("total_token_usage")
                    .map(|usage| count(usage, "total_tokens"));
                if total.is_some() && total == last_total {
                    continue;
                }
                last_total = total;
                let Some(usage) = info.get("last_token_usage") else {
                    continue;
                };
                let Some(timestamp) = entry
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(parse_timestamp)
                else {
                    continue;
                };
                if timestamp < since {
                    continue;
                }
                let input = count(usage, "input_tokens");
                let cached = count(usage, "cached_input_tokens").min(input);
                let row = UsageRow {
                    input: input - cached,
                    cache_read: cached,
                    output: count(usage, "output_tokens"),
                    ..UsageRow::default()
                };
                let name = if model.is_empty() { "codex" } else { &model };
                let key = format!(
                    "{}|{}|{}",
                    entry.get("timestamp").and_then(Value::as_str).unwrap_or(""),
                    info.get("total_token_usage").unwrap_or(&Value::Null),
                    usage,
                );
                requests.entry(key).or_insert_with(|| Request {
                    timestamp,
                    model: name.to_string(),
                    project: project.clone(),
                    usage: row,
                });
            }
            _ => {}
        }
    }
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.fff](Z|±HH:MM)` into Unix seconds.
fn parse_timestamp(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() < 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<i64> { text.get(range)?.parse().ok() };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut rest = &text[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        rest = fraction.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest {
        "" | "Z" | "z" => 0,
        zone if zone.len() == 6 && (zone.starts_with('+') || zone.starts_with('-')) => {
            let hours: i64 = zone.get(1..3)?.parse().ok()?;
            let minutes: i64 = zone.get(4..6)?.parse().ok()?;
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            sign * (hours * 3600 + minutes * 60)
        }
        _ => return None,
    };
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

const OPENROUTER_MODELS_URL: &str = "https://openrouter.ai/api/v1/models";
const PRICES_FILE: &str = "openrouter-prices.json";
/// How long a downloaded price list is used before it is fetched again.
const PRICES_MAX_AGE_SECS: u64 = 24 * 60 * 60;
const PRICES_TIMEOUT: Duration = Duration::from_secs(15);

/// One model's list price in US dollars per token.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPrice {
    /// OpenRouter id, such as `anthropic/claude-opus-5.5`.
    pub id: String,
    pub input: f64,
    pub output: f64,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    pub cache_write_1h: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPrices {
    pub models: Vec<ModelPrice>,
    /// When the list was downloaded, in Unix seconds.
    pub fetched_at: u64,
}

/// Anthropic and OpenAI list prices from OpenRouter's public model list.
///
/// Only the public list is downloaded; nothing about local usage is sent.
/// The list is cached on disk for a day, and a stale copy is returned when
/// the download fails.
#[tauri::command]
pub async fn provider_model_prices(app: AppHandle) -> Result<ModelPrices, String> {
    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|error| error.to_string())?
        .join(PRICES_FILE);
    tauri::async_runtime::spawn_blocking(move || model_prices(&cache, now_secs(), download_prices))
        .await
        .map_err(|error| error.to_string())?
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn model_prices(
    cache: &Path,
    now: u64,
    download: fn() -> Result<Vec<ModelPrice>, String>,
) -> Result<ModelPrices, String> {
    let cached = std::fs::read(cache)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ModelPrices>(&bytes).ok());
    if let Some(cached) = &cached {
        if now.saturating_sub(cached.fetched_at) < PRICES_MAX_AGE_SECS {
            return Ok(cached.clone());
        }
    }
    match download() {
        Ok(models) => {
            let prices = ModelPrices {
                models,
                fetched_at: now,
            };
            if let Some(dir) = cache.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(bytes) = serde_json::to_vec(&prices) {
                let _ = std::fs::write(cache, bytes);
            }
            Ok(prices)
        }
        Err(error) => cached.ok_or(error),
    }
}

fn download_prices() -> Result<Vec<ModelPrice>, String> {
    let agent = ureq::AgentBuilder::new().timeout(PRICES_TIMEOUT).build();
    let response = agent
        .get(OPENROUTER_MODELS_URL)
        .call()
        .map_err(|error| format!("Could not download model prices: {error}"))?;
    let body: Value = serde_json::from_reader(response.into_reader())
        .map_err(|error| format!("Model prices were not JSON: {error}"))?;
    let models = parse_openrouter_prices(&body);
    if models.is_empty() {
        return Err("Model price list was empty".into());
    }
    Ok(models)
}

/// Keeps Anthropic and OpenAI models with a usable price. OpenRouter sends
/// prices as decimal strings; "-1" and missing values mean unknown.
fn parse_openrouter_prices(body: &Value) -> Vec<ModelPrice> {
    let price = |pricing: &Value, key: &str| -> Option<f64> {
        let value = match pricing.get(key)? {
            Value::String(text) => text.parse::<f64>().ok()?,
            Value::Number(number) => number.as_f64()?,
            _ => return None,
        };
        (value >= 0.0 && value.is_finite()).then_some(value)
    };
    body.get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|model| {
            let id = model.get("id")?.as_str()?;
            // Variants such as `:free` or `:thinking` are not what the CLIs bill.
            let wanted =
                (id.starts_with("anthropic/") || id.starts_with("openai/")) && !id.contains(':');
            if !wanted {
                return None;
            }
            let pricing = model.get("pricing")?;
            Some(ModelPrice {
                id: id.to_string(),
                input: price(pricing, "prompt")?,
                output: price(pricing, "completion")?,
                cache_read: price(pricing, "input_cache_read"),
                cache_write: price(pricing, "input_cache_write"),
                cache_write_1h: price(pricing, "input_cache_write_1h"),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, lines: &[&str]) -> PathBuf {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, lines.join("\n")).unwrap();
        path
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("monocode-usage-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_utc_and_offset_timestamps() {
        assert_eq!(parse_timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_timestamp("2026-09-29T06:38:45.123Z"),
            Some(1_790_663_925)
        );
        assert_eq!(
            parse_timestamp("2026-09-29T12:08:45+05:30"),
            Some(1_790_663_925)
        );
        assert_eq!(parse_timestamp("2024-02-29T00:00:00Z"), Some(1_709_164_800));
        assert_eq!(parse_timestamp("not a time"), None);
        assert_eq!(parse_timestamp("2026-13-01T00:00:00Z"), None);
    }

    #[test]
    fn claude_counts_each_request_once_and_splits_cache_writes() {
        let dir = temp_dir("claude");
        let usage = r#""usage":{"input_tokens":2,"cache_creation_input_tokens":300,"cache_read_input_tokens":4000,"output_tokens":50,"cache_creation":{"ephemeral_5m_input_tokens":100,"ephemeral_1h_input_tokens":200}}"#;
        let block = |ts: &str| {
            format!(
                r#"{{"type":"assistant","timestamp":"{ts}","cwd":"/work/app","requestId":"req_1","message":{{"id":"msg_1","model":"claude-opus-5-5",{usage}}}}}"#
            )
        };
        let first = block("2026-09-29T06:01:00Z");
        let second = block("2026-09-29T06:01:01Z");
        let synthetic = r#"{"type":"assistant","timestamp":"2026-09-29T06:02:00Z","message":{"id":"x","model":"<synthetic>","usage":{"input_tokens":9,"output_tokens":9}}}"#;
        let old = r#"{"type":"assistant","timestamp":"2020-01-01T00:00:00Z","requestId":"r0","message":{"id":"m0","model":"claude-opus-5-5","usage":{"input_tokens":9,"output_tokens":9}}}"#;
        write(
            &dir,
            "p/session.jsonl",
            &[&first, &second, synthetic, old, "{broken"],
        );
        // A resumed session repeats the same request in a second transcript.
        write(&dir, "p/resumed/subagents/agent.jsonl", &[&first]);

        let report = scan(
            std::slice::from_ref(&dir),
            parse_timestamp("2026-09-01T00:00:00Z").unwrap(),
            parse_claude_file,
        );
        assert!(report.found);
        assert_eq!(report.files_scanned, 2);
        assert_eq!(
            report.rows,
            vec![UsageRow {
                slot: parse_timestamp("2026-09-29T06:00:00Z").unwrap(),
                model: "claude-opus-5-5".into(),
                project: "/work/app".into(),
                input: 2,
                cache_read: 4000,
                cache_write_5m: 100,
                cache_write_1h: 200,
                output: 50,
            }]
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn codex_uses_per_response_usage_and_skips_repeats() {
        let dir = temp_dir("codex");
        let count = |ts: &str, total: u64, input: u64| {
            format!(
                r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"total_tokens":{total}}},"last_token_usage":{{"input_tokens":{input},"cached_input_tokens":800,"output_tokens":40,"total_tokens":{}}}}}}}}}"#,
                input + 40
            )
        };
        write(
            &dir,
            "2026/09/29/rollout-a.jsonl",
            &[
                r#"{"timestamp":"2026-09-29T06:00:00Z","type":"session_meta","payload":{"cwd":"/work/api"}}"#,
                r#"{"timestamp":"2026-09-29T06:00:01Z","type":"turn_context","payload":{"cwd":"/work/api","model":"gpt-5.5-codex"}}"#,
                r#"{"timestamp":"2026-09-29T06:00:02Z","type":"event_msg","payload":{"type":"token_count","info":null}}"#,
                &count("2026-09-29T06:00:03Z", 1040, 1000),
                &count("2026-09-29T06:00:04Z", 1040, 1000),
                &count("2026-09-29T06:20:00Z", 2080, 1000),
            ],
        );

        let report = scan(std::slice::from_ref(&dir), 0, parse_codex_file);
        let rows: Vec<(i64, u64, u64, u64)> = report
            .rows
            .iter()
            .map(|row| (row.slot, row.input, row.cache_read, row.output))
            .collect();
        let six = parse_timestamp("2026-09-29T06:00:00Z").unwrap();
        assert_eq!(rows, vec![(six, 200, 800, 40), (six + 900, 200, 800, 40)]);
        assert!(report
            .rows
            .iter()
            .all(|row| row.model == "gpt-5.5-codex" && row.project == "/work/api"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn claude_keeps_the_final_snapshot_of_a_request() {
        let dir = temp_dir("claude-snapshot");
        let line = |ts: &str, output: u64| {
            format!(
                r#"{{"type":"assistant","timestamp":"{ts}","requestId":"req_1","message":{{"id":"msg_1","model":"claude-opus-5-5","usage":{{"input_tokens":5,"cache_read_input_tokens":100,"output_tokens":{output}}}}}}}"#
            )
        };
        write(
            &dir,
            "p/session.jsonl",
            &[
                &line("2026-09-29T06:01:00Z", 1),
                &line("2026-09-29T06:01:09Z", 420),
            ],
        );
        let report = scan(std::slice::from_ref(&dir), 0, parse_claude_file);
        let rows: Vec<(u64, u64, u64)> = report
            .rows
            .iter()
            .map(|row| (row.input, row.cache_read, row.output))
            .collect();
        assert_eq!(rows, vec![(5, 100, 420)]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn codex_reads_archived_and_compressed_sessions() {
        let dir = temp_dir("codex-archive");
        // Each session's response lands at its own moment.
        let session = |model: &str, second: u32| {
            [
                format!(
                    r#"{{"timestamp":"2026-09-29T06:00:00Z","type":"turn_context","payload":{{"cwd":"/work/api","model":"{model}"}}}}"#
                ),
                format!(
                    r#"{{"timestamp":"2026-09-29T06:00:{second:02}Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"total_tokens":110}},"last_token_usage":{{"input_tokens":100,"output_tokens":10}}}}}}}}"#
                ),
            ]
            .join("\n")
        };
        let sessions = dir.join("sessions");
        let archived = dir.join("archived_sessions");
        write(
            &sessions,
            "2026/09/29/rollout-live.jsonl",
            &[&session("gpt-live", 1)],
        );
        write(
            &archived,
            "rollout-archived.jsonl",
            &[&session("gpt-archived", 2)],
        );
        std::fs::write(
            archived.join("rollout-packed.jsonl.zst"),
            ruzstd::encoding::compress_to_vec(
                session("gpt-packed", 3).as_bytes(),
                ruzstd::encoding::CompressionLevel::Fastest,
            ),
        )
        .unwrap();
        // Caught mid-move: the same rollout under both roots counts once.
        write(
            &sessions,
            "2026/09/29/rollout-moved.jsonl",
            &[&session("gpt-moved", 4)],
        );
        write(
            &archived,
            "rollout-moved.jsonl",
            &[&session("gpt-moved", 4)],
        );

        let report = scan(&[sessions, archived], 0, parse_codex_file);
        let mut models: Vec<(&str, u64)> = report
            .rows
            .iter()
            .map(|row| (row.model.as_str(), row.input))
            .collect();
        models.sort();
        assert_eq!(
            models,
            vec![
                ("gpt-archived", 100),
                ("gpt-live", 100),
                ("gpt-moved", 100),
                ("gpt-packed", 100),
            ]
        );
        assert_eq!(report.files_scanned, 4);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn codex_counts_a_replayed_response_once() {
        let dir = temp_dir("codex-replay");
        let meta = r#"{"timestamp":"2026-09-29T06:00:00Z","type":"turn_context","payload":{"cwd":"/work/api","model":"gpt-5.5"}}"#;
        let response = |ts: &str, total: u64| {
            format!(
                r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"total_tokens":{total}}},"last_token_usage":{{"input_tokens":100,"output_tokens":10}}}}}}}}"#
            )
        };
        let first = response("2026-09-29T06:00:01Z", 110);
        write(&dir, "2026/09/29/rollout-a.jsonl", &[meta, &first]);
        // A resumed session copies the earlier response, then adds its own.
        write(
            &dir,
            "2026/09/29/rollout-b.jsonl",
            &[meta, &first, &response("2026-09-29T06:05:00Z", 220)],
        );
        let report = scan(std::slice::from_ref(&dir), 0, parse_codex_file);
        let input: u64 = report.rows.iter().map(|row| row.input).sum();
        assert_eq!(input, 200);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn reads_every_frame_of_a_compressed_log() {
        let dir = temp_dir("zstd-frames");
        let compress = |text: &str| {
            ruzstd::encoding::compress_to_vec(
                text.as_bytes(),
                ruzstd::encoding::CompressionLevel::Fastest,
            )
        };
        let mut bytes = compress("one\ntwo\n");
        bytes.extend(compress("three\n"));
        let path = dir.join("log.jsonl.zst");
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(lines(&path).collect::<Vec<_>>(), ["one", "two", "three"]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_log_directory_is_not_found() {
        let report = scan(
            &[PathBuf::from("/definitely/not/here")],
            0,
            parse_codex_file,
        );
        assert!(!report.found);
        assert!(report.rows.is_empty());
    }

    #[test]
    fn keeps_priced_anthropic_and_openai_models() {
        let body = serde_json::json!({"data": [
            {"id": "anthropic/claude-opus-5.5", "pricing": {"prompt": "0.000004", "completion": "0.00002", "input_cache_read": "0.0000002", "input_cache_write": "0.000005"}},
            {"id": "openai/gpt-5.5", "pricing": {"prompt": "0.000005", "completion": "0.00003"}},
            {"id": "anthropic/claude-opus-5.5:thinking", "pricing": {"prompt": "1", "completion": "1"}},
            {"id": "google/gemini-3-pro", "pricing": {"prompt": "0.000002", "completion": "0.00001"}},
            {"id": "openai/auto", "pricing": {"prompt": "-1", "completion": "-1"}}
        ]});
        let models = parse_openrouter_prices(&body);
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["anthropic/claude-opus-5.5", "openai/gpt-5.5"]
        );
        assert_eq!(models[0].cache_write, Some(0.000005));
        assert_eq!(models[0].cache_write_1h, None);
        assert_eq!(models[1].cache_read, None);
    }

    #[test]
    fn prices_come_from_cache_until_stale_then_fall_back_to_it() {
        let dir = temp_dir("prices");
        let cache = dir.join("nested").join(PRICES_FILE);
        let price = |id: &str| ModelPrice {
            id: id.into(),
            input: 1.0,
            output: 2.0,
            cache_read: None,
            cache_write: None,
            cache_write_1h: None,
        };
        fn fresh() -> Result<Vec<ModelPrice>, String> {
            Ok(vec![ModelPrice {
                id: "openai/fresh".into(),
                input: 1.0,
                output: 2.0,
                cache_read: None,
                cache_write: None,
                cache_write_1h: None,
            }])
        }
        fn offline() -> Result<Vec<ModelPrice>, String> {
            Err("offline".into())
        }

        assert_eq!(model_prices(&cache, 1_000, offline).unwrap_err(), "offline");
        let first = model_prices(&cache, 1_000, fresh).unwrap();
        assert_eq!(
            (first.models[0].id.as_str(), first.fetched_at),
            ("openai/fresh", 1_000)
        );

        std::fs::write(
            &cache,
            serde_json::to_vec(&ModelPrices {
                models: vec![price("openai/cached")],
                fetched_at: 1_000,
            })
            .unwrap(),
        )
        .unwrap();
        // Within a day the cached copy is used without downloading.
        assert_eq!(
            model_prices(&cache, 2_000, fresh).unwrap().models[0].id,
            "openai/cached"
        );
        // Once stale, a failed download still returns the old copy.
        let stale = 1_000 + PRICES_MAX_AGE_SECS;
        assert_eq!(
            model_prices(&cache, stale, offline).unwrap().models[0].id,
            "openai/cached"
        );
        assert_eq!(
            model_prices(&cache, stale, fresh).unwrap().models[0].id,
            "openai/fresh"
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
