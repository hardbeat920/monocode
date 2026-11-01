//! MonoMods: discovery, consent, and the privileged calls mods make through
//! the host. Rust is the trust boundary: every call re-reads the installed
//! manifest and the consent record, so the frontend's copy is display only.
//! See docs/monomods.md.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};
use tauri::{AppHandle, Manager, Runtime};

use crate::fs::path_to_js;

/// Scheme that serves the bootstrap page to each mod's hidden frame.
pub const SCHEME: &str = "monomod";

const BOOTSTRAP: &str = include_str!("../../src/features/mods/runtime/bootstrap.html");

/// The frame may only start blob workers; nothing in it can fetch or frame.
const CSP: &str = "sandbox allow-scripts; default-src 'none'; \
    script-src 'unsafe-inline' blob:; worker-src blob:; connect-src 'none'";

const MAX_STDOUT: usize = 1024 * 1024;
const MAX_READ: u64 = 2 * 1024 * 1024;
const MAX_STORE: usize = 1024 * 1024;
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// Serialises read-modify-write of the consent, unpacked and store files.
static FILES: Mutex<()> = Mutex::new(());

#[derive(Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
struct Permissions {
    #[serde(default)]
    process: Vec<String>,
    #[serde(default)]
    fs: FsPermissions,
    #[serde(default)]
    http: Vec<String>,
    #[serde(default)]
    env: Vec<String>,
}

#[derive(Deserialize, Default, Clone)]
struct FsPermissions {
    #[serde(default)]
    read: Vec<String>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    name: String,
    api_version: u32,
    entry: String,
    #[serde(default)]
    permissions: Permissions,
}

struct Found {
    dir: PathBuf,
    source: &'static str,
    name: String,
    hash: String,
    raw: Value,
    manifest: Result<Manifest, String>,
    /// Folders of later mods with the same name, hidden by this one.
    shadows: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEntry {
    name: String,
    dir: String,
    /// "project", "user" or "unpacked".
    source: &'static str,
    manifest: Value,
    error: Option<String>,
    enabled: bool,
    manifest_hash: String,
    shadows: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Consent {
    name: String,
    hash: String,
}

fn mods_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("mods"))
}

fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn is_valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn parse_manifest(raw: &Value) -> Result<Manifest, String> {
    let manifest: Manifest =
        serde_json::from_value(raw.clone()).map_err(|e| format!("Invalid mod.json: {e}"))?;
    if !is_valid_name(&manifest.name) {
        return Err(format!("Invalid mod name \"{}\"", manifest.name));
    }
    if manifest.api_version != 1 {
        return Err(format!("Unsupported apiVersion {}", manifest.api_version));
    }
    if confined(Path::new("/"), &manifest.entry).is_none() {
        return Err("entry must be a relative path inside the mod".into());
    }
    Ok(manifest)
}

fn read_found(dir: PathBuf, source: &'static str) -> Option<Found> {
    let bytes = std::fs::read(dir.join("mod.json")).ok()?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let folder = dir.file_name()?.to_string_lossy().into_owned();
    let (raw, manifest) = match serde_json::from_slice::<Value>(&bytes) {
        Ok(raw) => {
            let manifest = parse_manifest(&raw);
            (raw, manifest)
        }
        Err(e) => (Value::Null, Err(format!("Invalid mod.json: {e}"))),
    };
    let name = manifest.as_ref().map(|m| m.name.clone()).unwrap_or(folder);
    Some(Found {
        dir: std::fs::canonicalize(&dir).unwrap_or(dir),
        source,
        name,
        hash,
        raw,
        manifest,
        shadows: Vec::new(),
    })
}

/// Project mods, then user mods, then unpacked folders; the first name wins.
fn discover<R: Runtime>(app: &AppHandle<R>, project: Option<&str>) -> Vec<Found> {
    let mut roots: Vec<(PathBuf, &'static str)> = Vec::new();
    if let Some(project) = project.filter(|p| !p.is_empty()) {
        roots.push((Path::new(project).join(".monocode/mods"), "project"));
    }
    if let Some(home) = crate::dirs_home() {
        roots.push((Path::new(&home).join(".monocode/mods"), "user"));
    }
    let mut dirs: Vec<(PathBuf, &'static str)> = Vec::new();
    for (root, source) in roots {
        let Ok(reader) = std::fs::read_dir(&root) else {
            continue;
        };
        let mut children: Vec<PathBuf> = reader.flatten().map(|e| e.path()).collect();
        children.sort();
        dirs.extend(children.into_iter().map(|dir| (dir, source)));
    }
    if let Ok(base) = mods_dir(app) {
        let unpacked: Vec<String> = read_json(&base.join("unpacked.json"));
        dirs.extend(unpacked.into_iter().map(|p| (PathBuf::from(p), "unpacked")));
    }
    let mut out: Vec<Found> = Vec::new();
    for (dir, source) in dirs {
        if let Some(found) = read_found(dir, source) {
            match out.iter_mut().find(|f| f.name == found.name) {
                Some(winner) => winner.shadows.push(path_to_js(&found.dir)),
                None => out.push(found),
            }
        }
    }
    out
}

fn consents<R: Runtime>(app: &AppHandle<R>) -> HashMap<String, Consent> {
    mods_dir(app)
        .map(|base| read_json(&base.join("consent.json")))
        .unwrap_or_default()
}

fn is_enabled(found: &Found, consents: &HashMap<String, Consent>) -> bool {
    found.manifest.is_ok()
        && consents
            .get(&path_to_js(&found.dir))
            .is_some_and(|c| c.hash == found.hash)
}

/// The mod's folder and manifest, if the user consented to this exact manifest.
fn authorize<R: Runtime>(
    app: &AppHandle<R>,
    project: Option<&str>,
    name: &str,
) -> Result<(PathBuf, Manifest), String> {
    let consents = consents(app);
    let found = discover(app, project)
        .into_iter()
        .find(|f| f.name == name)
        .ok_or_else(|| format!("Mod {name} is not installed"))?;
    if !is_enabled(&found, &consents) {
        return Err(format!("Mod {name} is not enabled"));
    }
    let manifest = found.manifest?;
    Ok((found.dir, manifest))
}

#[tauri::command(async)]
pub fn mods_list(app: AppHandle, project: Option<String>) -> Vec<ModEntry> {
    let consents = consents(&app);
    discover(&app, project.as_deref())
        .into_iter()
        .map(|found| ModEntry {
            enabled: is_enabled(&found, &consents),
            name: found.name,
            dir: path_to_js(&found.dir),
            source: found.source,
            manifest: found.raw,
            error: found.manifest.err(),
            manifest_hash: found.hash,
            shadows: found.shadows,
        })
        .collect()
}

/// Records consent for the manifest the user saw; a changed manifest asks again.
#[tauri::command(async)]
pub fn mods_set_enabled(
    app: AppHandle,
    project: Option<String>,
    name: String,
    enabled: bool,
    manifest_hash: String,
) -> Result<(), String> {
    let found = discover(&app, project.as_deref())
        .into_iter()
        .find(|f| f.name == name)
        .ok_or_else(|| format!("Mod {name} is not installed"))?;
    if let Err(error) = &found.manifest {
        return Err(error.clone());
    }
    if enabled && found.hash != manifest_hash {
        return Err("mod.json changed; review its permissions again".into());
    }
    let _guard = FILES.lock().map_err(|e| e.to_string())?;
    let path = mods_dir(&app)?.join("consent.json");
    let mut consents: HashMap<String, Consent> = read_json(&path);
    let key = path_to_js(&found.dir);
    if enabled {
        consents.insert(
            key,
            Consent {
                name,
                hash: found.hash,
            },
        );
    } else {
        consents.remove(&key);
    }
    write_json(&path, &consents)
}

/// Remembers a folder for "Load unpacked".
#[tauri::command(async)]
pub fn mods_add_unpacked(app: AppHandle, path: String) -> Result<(), String> {
    let dir = std::fs::canonicalize(&path).map_err(|e| format!("{path}: {e}"))?;
    if !dir.join("mod.json").is_file() {
        return Err("The folder has no mod.json".into());
    }
    let _guard = FILES.lock().map_err(|e| e.to_string())?;
    let file = mods_dir(&app)?.join("unpacked.json");
    let mut unpacked: Vec<String> = read_json(&file);
    let dir = path_to_js(&dir);
    if !unpacked.contains(&dir) {
        unpacked.push(dir);
    }
    write_json(&file, &unpacked)
}

/// `rel` joined onto `dir`, refusing absolute paths and `..`.
fn confined(dir: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    rel.components()
        .all(|c| {
            matches!(
                c,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
        .then(|| dir.join(rel))
}

/// For the bundler: a source file inside the mod folder.
#[tauri::command(async)]
pub fn mods_read_source(
    app: AppHandle,
    project: Option<String>,
    name: String,
    rel_path: String,
) -> Result<String, String> {
    let (dir, _) = authorize(&app, project.as_deref(), &name)?;
    let path = confined(&dir, &rel_path).ok_or("Path escapes the mod folder")?;
    let real = std::fs::canonicalize(&path).map_err(|e| format!("{rel_path}: {e}"))?;
    if !real.starts_with(&dir) {
        return Err("Path escapes the mod folder".into());
    }
    read_text(&real)
}

fn read_text(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err("Not a file".into());
    }
    if meta.len() > MAX_READ {
        return Err("File is too large".into());
    }
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// A bare name from `permissions.process`. Paths are refused, so a mod cannot
/// run its own `bin/git`; the name is looked up on the login-shell PATH.
fn process_allowed(permissions: &Permissions, argv0: &str) -> bool {
    let name = argv0.strip_suffix(".exe").unwrap_or(argv0);
    !name.is_empty() && !name.contains(['/', '\\']) && permissions.process.iter().any(|p| p == name)
}

/// Variables that make common tools run other code (pagers, editors, ssh,
/// startup files, config locations).
const EXEC_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "SHELL",
    "ENV",
    "BASH_ENV",
    "ZDOTDIR",
    "PROMPT_COMMAND",
    "IFS",
    "EDITOR",
    "VISUAL",
    "PAGER",
    "BROWSER",
    "XDG_CONFIG_HOME",
    "XDG_CONFIG_DIRS",
    "GH_PAGER",
    "GH_EDITOR",
    "GH_BROWSER",
    "GH_CONFIG_DIR",
    "SSH_ASKPASS",
    "NODE_OPTIONS",
    "NODE_PATH",
    "PYTHONPATH",
    "PYTHONSTARTUP",
    "PYTHONHOME",
    "PERL5OPT",
    "PERL5LIB",
    "PERLLIB",
    "RUBYOPT",
    "RUBYLIB",
];

/// Plain names only, and nothing that changes which code the binary loads.
fn env_key_allowed(key: &str) -> bool {
    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !EXEC_ENV.contains(&key)
        && !key.starts_with("LD_")
        && !key.starts_with("DYLD_")
        // GIT_SSH_COMMAND, GIT_CONFIG_*, GIT_DIR, GIT_EXEC_PATH and friends. Arguments
        // (`-c alias.x=!cmd`) still run code, so the consent sheet treats git as full access.
        && !key.starts_with("GIT_")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessResult {
    exit_code: i32,
    stdout: String,
    stderr: String,
    is_stdout_truncated: bool,
}

#[tauri::command]
pub async fn mod_exec(
    app: AppHandle,
    project: Option<String>,
    name: String,
    argv: Vec<String>,
    cwd: Option<String>,
    timeout_ms: Option<u64>,
    env: Option<HashMap<String, String>>,
) -> Result<ProcessResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (_, manifest) = authorize(&app, project.as_deref(), &name)?;
        let argv0 = argv.first().ok_or("argv is empty")?;
        if !process_allowed(&manifest.permissions, argv0) {
            return Err(format!("{name} may not run {argv0}"));
        }
        let binary =
            crate::harness::resolve_gui_binary(argv0).ok_or(format!("{argv0}: not found"))?;
        let binary = binary.to_string_lossy().into_owned();
        let env = env.unwrap_or_default();
        if let Some(key) = env.keys().find(|k| !env_key_allowed(k)) {
            return Err(format!("env {key} is not allowed"));
        }
        // ponytail: exec_output takes no env, so vars go through `env`; Windows ignores them.
        let (command, args) = if env.is_empty() || cfg!(windows) {
            (binary, argv[1..].to_vec())
        } else {
            let mut args: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
            args.push(binary);
            args.extend_from_slice(&argv[1..]);
            ("/usr/bin/env".to_string(), args)
        };
        let timeout = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS).max(1));
        let output = crate::harness::exec_output(&command, &args, cwd.as_deref(), timeout)?;
        let is_stdout_truncated = output.stdout.len() > MAX_STDOUT;
        let stdout = &output.stdout[..output.stdout.len().min(MAX_STDOUT)];
        let stderr = &output.stderr[..output.stderr.len().min(MAX_STDOUT)];
        Ok(ProcessResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(stdout).into_owned(),
            stderr: String::from_utf8_lossy(stderr).into_owned(),
            is_stdout_truncated,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// `*` and `?` match within one segment, `**` across any number of them.
fn glob_match(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|i| glob_match(rest, &path[i..])),
        Some((seg, rest)) => path.split_first().is_some_and(|(head, tail)| {
            segment_match(seg.as_bytes(), head.as_bytes()) && glob_match(rest, tail)
        }),
    }
}

fn segment_match(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|i| segment_match(rest, &text[i..])),
        Some((b'?', rest)) => !text.is_empty() && segment_match(rest, &text[1..]),
        Some((c, rest)) => text.first() == Some(c) && segment_match(rest, &text[1..]),
    }
}

fn segments(path: &str) -> Vec<&str> {
    path.split(['/', '\\']).filter(|s| !s.is_empty()).collect()
}

/// Whether a canonical path matches one of the read globs. `$PROJECT` and
/// `$HOME` expand to canonical paths; a glob naming an absent root never matches.
fn fs_allowed(globs: &[String], path: &Path, project: Option<&Path>, home: Option<&Path>) -> bool {
    let path = path_to_js(path);
    let path = segments(&path);
    globs.iter().any(|glob| {
        let expanded = if let Some(rest) = glob.strip_prefix("$PROJECT") {
            project.map(|p| format!("{}{rest}", path_to_js(p)))
        } else if let Some(rest) = glob.strip_prefix("$HOME") {
            home.map(|h| format!("{}{rest}", path_to_js(h)))
        } else {
            Path::new(glob).is_absolute().then(|| glob.clone())
        };
        expanded.is_some_and(|g| glob_match(&segments(&g), &path))
    })
}

fn mtime_ms(meta: &std::fs::Metadata) -> f64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

fn kind(meta: &std::fs::Metadata) -> &'static str {
    if meta.is_dir() {
        "dir"
    } else if meta.is_file() {
        "file"
    } else {
        "other"
    }
}

/// `read`, `list` or `stat` on a path the mod's `fs.read` globs cover.
#[tauri::command(async)]
pub fn mod_fs(
    app: AppHandle,
    project: Option<String>,
    name: String,
    op: String,
    path: String,
) -> Result<Value, String> {
    let (_, manifest) = authorize(&app, project.as_deref(), &name)?;
    if !Path::new(&path).is_absolute() {
        return Err("Path must be absolute".into());
    }
    let real = std::fs::canonicalize(&path).map_err(|e| format!("{path}: {e}"))?;
    let project = project.and_then(|p| std::fs::canonicalize(p).ok());
    let home = crate::dirs_home().and_then(|h| std::fs::canonicalize(h).ok());
    if !fs_allowed(
        &manifest.permissions.fs.read,
        &real,
        project.as_deref(),
        home.as_deref(),
    ) {
        return Err(format!("{name} may not read {path}"));
    }
    match op.as_str() {
        "read" => read_text(&real).map(Value::String),
        "stat" => {
            let meta = std::fs::metadata(&real).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "kind": kind(&meta), "mtimeMs": mtime_ms(&meta) }))
        }
        "list" => {
            let reader = std::fs::read_dir(&real).map_err(|e| format!("{path}: {e}"))?;
            let entries: Vec<Value> = reader
                .flatten()
                .filter_map(|entry| {
                    let link = entry.file_type().ok()?.is_symlink();
                    let meta = std::fs::metadata(entry.path()).ok()?;
                    Some(serde_json::json!({
                        "name": entry.file_name().to_string_lossy(),
                        "kind": kind(&meta),
                        "mtimeMs": mtime_ms(&meta),
                        "isLink": link,
                    }))
                })
                .collect();
            Ok(Value::Array(entries))
        }
        _ => Err(format!("Unknown fs op {op}")),
    }
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || a == 0
                || (a == 100 && (64..128).contains(&b)))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

/// Resolves, then drops private addresses, so DNS cannot point a public
/// hostname at the local network.
fn resolve_public(addr: &str) -> std::io::Result<Vec<SocketAddr>> {
    let found: Vec<SocketAddr> = addr
        .to_socket_addrs()?
        .filter(|a| is_public_ip(a.ip()))
        .collect();
    if found.is_empty() {
        return Err(std::io::Error::other("Private addresses are not allowed"));
    }
    Ok(found)
}

#[derive(Serialize)]
pub struct HttpResponse {
    status: u16,
    headers: HashMap<String, String>,
    body: String,
}

#[tauri::command]
pub async fn mod_http(
    app: AppHandle,
    project: Option<String>,
    name: String,
    method: String,
    url: String,
    headers: Option<HashMap<String, String>>,
    body: Option<String>,
) -> Result<HttpResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (_, manifest) = authorize(&app, project.as_deref(), &name)?;
        let parsed = url::Url::parse(&url).map_err(|e| e.to_string())?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("Only http(s) URLs are allowed".into());
        }
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if !manifest
            .permissions
            .http
            .iter()
            .any(|h| h.eq_ignore_ascii_case(&host))
        {
            return Err(format!("{name} may not reach {host}"));
        }
        if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
            return Err(format!("Unsupported method {method}"));
        }
        // No redirects: they could leave the allowlist.
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(DEFAULT_TIMEOUT_MS))
            .redirects(0)
            .resolver(resolve_public)
            .build();
        let mut request = agent.request(&method, parsed.as_str());
        for (key, value) in headers.iter().flatten() {
            request = request.set(key, value);
        }
        let response = match body {
            Some(payload) => request.send_string(&payload),
            None => request.call(),
        };
        let response = match response {
            Ok(response) | Err(ureq::Error::Status(_, response)) => response,
            Err(error) => return Err(error.to_string()),
        };
        let headers = response
            .headers_names()
            .into_iter()
            .filter_map(|k| response.header(&k).map(|v| (k.clone(), v.to_string())))
            .collect();
        let status = response.status();
        let body = response.into_string().map_err(|e| e.to_string())?;
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// `get`, `set` or `delete` in the mod's JSON store.
#[tauri::command(async)]
pub fn mod_store(
    app: AppHandle,
    project: Option<String>,
    name: String,
    op: String,
    key: String,
    value: Option<Value>,
) -> Result<Option<Value>, String> {
    let (dir, _) = authorize(&app, project.as_deref(), &name)?;
    let _guard = FILES.lock().map_err(|e| e.to_string())?;
    // Keyed by folder too, so a same-named mod elsewhere cannot read this one's data.
    let folder = format!("{:x}", Sha256::digest(path_to_js(&dir).as_bytes()));
    let path = mods_dir(&app)?
        .join(&name)
        .join(format!("store-{}.json", &folder[..16]));
    let mut store: Map<String, Value> = read_json(&path);
    match op.as_str() {
        "get" => return Ok(store.get(&key).cloned()),
        "set" => {
            store.insert(key, value.unwrap_or(Value::Null));
        }
        "delete" => {
            store.remove(&key);
        }
        _ => return Err(format!("Unknown store op {op}")),
    }
    let size = serde_json::to_vec(&store).map_err(|e| e.to_string())?.len();
    if size > MAX_STORE {
        return Err("Store is full (1 MB)".into());
    }
    write_json(&path, &store)?;
    Ok(None)
}

/// ponytail: reads the app's own environment, not the login shell's.
#[tauri::command(async)]
pub fn mod_env(
    app: AppHandle,
    project: Option<String>,
    name: String,
    key: String,
) -> Result<Option<String>, String> {
    let (_, manifest) = authorize(&app, project.as_deref(), &name)?;
    if !manifest.permissions.env.contains(&key) {
        return Ok(None);
    }
    Ok(std::env::var(&key).ok())
}

/// Serves `monomod://localhost/<name>` for enabled mods only.
pub fn serve<R: Runtime>(
    app: &AppHandle<R>,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let not_found = || {
        tauri::http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default()
    };
    if request.method() != tauri::http::Method::GET {
        return not_found();
    }
    let name = request.uri().path().trim_start_matches('/');
    if !is_valid_name(name) || !consents(app).values().any(|c| c.name == name) {
        return not_found();
    }
    tauri::http::Response::builder()
        .status(200)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Content-Security-Policy", CSP)
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .body(BOOTSTRAP.as_bytes().to_vec())
        .unwrap_or_else(|_| not_found())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perms(process: &[&str]) -> Permissions {
        Permissions {
            process: process.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn process_matches_bare_names_only() {
        let p = perms(&["git", "gh"]);
        assert!(process_allowed(&p, "git"));
        assert!(process_allowed(&p, "gh.exe"));
        assert!(!process_allowed(&p, "/opt/homebrew/bin/gh"));
        assert!(!process_allowed(&p, "/repo/.monocode/mods/x/bin/git"));
        assert!(!process_allowed(&p, "./git"));
        assert!(!process_allowed(&p, r"C:\bin\gh.exe"));
        assert!(!process_allowed(&p, "bash"));
        assert!(!process_allowed(&p, "gitx"));
        assert!(!process_allowed(&p, ""));
        assert!(!process_allowed(&perms(&[]), "git"));
    }

    #[test]
    fn env_keys_cannot_change_loaded_code() {
        assert!(env_key_allowed("GH_TOKEN"));
        assert!(!env_key_allowed("PATH"));
        assert!(!env_key_allowed("DYLD_INSERT_LIBRARIES"));
        assert!(!env_key_allowed("LD_PRELOAD"));
        assert!(!env_key_allowed("GIT_SSH_COMMAND"));
        assert!(!env_key_allowed("GIT_CONFIG_COUNT"));
        assert!(!env_key_allowed("BASH_ENV"));
        assert!(!env_key_allowed("NODE_OPTIONS"));
        assert!(!env_key_allowed("HOME"));
        assert!(!env_key_allowed("-i"));
        assert!(!env_key_allowed("A=B"));
    }

    #[test]
    fn fs_globs_expand_and_match() {
        let project = Path::new("/work/app");
        let home = Path::new("/home/me");
        let globs = vec!["$PROJECT/**".to_string(), "$HOME/.gitconfig".to_string()];
        let ok = |p: &str| fs_allowed(&globs, Path::new(p), Some(project), Some(home));
        assert!(ok("/work/app"));
        assert!(ok("/work/app/src/main.ts"));
        assert!(ok("/home/me/.gitconfig"));
        assert!(!ok("/work/application/x"));
        assert!(!ok("/work"));
        assert!(!ok("/home/me/.ssh/id_rsa"));
        assert!(!fs_allowed(
            &globs,
            Path::new("/work/app/x"),
            None,
            Some(home)
        ));
        let star = vec!["/etc/*.conf".to_string()];
        assert!(fs_allowed(&star, Path::new("/etc/a.conf"), None, None));
        assert!(!fs_allowed(&star, Path::new("/etc/sub/a.conf"), None, None));
        assert!(!fs_allowed(
            &["relative/**".to_string()],
            Path::new("/relative/x"),
            None,
            None
        ));
    }

    #[test]
    fn private_addresses_are_refused() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "192.168.1.1",
            "172.16.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["140.82.112.3", "2606:4700::1111"] {
            assert!(is_public_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn sources_stay_inside_the_mod() {
        let dir = Path::new("/mods/radar");
        assert_eq!(
            confined(dir, "src/register.tsx"),
            Some(dir.join("src/register.tsx"))
        );
        assert_eq!(confined(dir, "../pulse/x"), None);
        assert_eq!(confined(dir, "/etc/passwd"), None);
    }

    #[test]
    fn manifests_are_validated() {
        let ok = serde_json::json!({ "name": "radar", "version": "0.1.0", "apiVersion": 1, "entry": "src/register.tsx" });
        assert!(parse_manifest(&ok).is_ok());
        let mut bad = ok.clone();
        bad["name"] = "Radar!".into();
        assert!(parse_manifest(&bad).is_err());
        let mut bad = ok.clone();
        bad["apiVersion"] = 2.into();
        assert!(parse_manifest(&bad).is_err());
        let mut bad = ok;
        bad["entry"] = "../x.ts".into();
        assert!(parse_manifest(&bad).is_err());
    }
}
