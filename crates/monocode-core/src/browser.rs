//! The browser module (`monocode_browser`) in Monocode Native: starting it,
//! its own loopback endpoint, the state the UI follows, and what each turn
//! of an agent gets: the session's credentials in the environment and the
//! `monocode-browser` MCP server for providers that take MCP servers at
//! launch, or the CLI instructions for the others.
//!
//! Unlike the Tauri app, nothing is written to the user's provider
//! configuration: like [`crate::visual_bridge`], the server is given to each
//! launch (Claude `--mcp-config`, Codex `-c mcp_servers.…`, ACP
//! `mcpServers`), and the others reach the CLI through their shell, which
//! inherits the session's environment.

use std::collections::HashMap;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

use monocode_browser::agent::register::{self, Tools};
use monocode_browser::agent::server_binary;
use monocode_browser::protocol::{ENV_ENDPOINT, ENV_TOKEN};
use monocode_browser::turn::TurnAccess;
pub use monocode_browser::BrowserHost;
use monocode_browser::Embedder;
use rusqlite::Connection;
use serde_json::{json, Value};

use crate::provider::Kind;

mod live_pane;
pub use live_pane::{queue_input, Ended, LivePane};

pub use monocode_browser::protocol::{
    AccessMode, BrowserMode, BrowserSettings, BrowserState, ConnectionState, TabOverview,
};
pub use monocode_browser::{devservers, live, pick, protocol, targets, turn, view};

/// The window grants and requests belong to: Native has one main window.
pub const WINDOW: &str = "main";
/// The MCP server's name in a provider's launch configuration.
pub const SERVER_NAME: &str = monocode_browser::agent::mcp::SERVER_NAME;
/// The subcommand that runs the MCP server.
pub const MCP_SUBCOMMAND: &str = "browser-mcp";
/// Codex's per-call limit for the server (its default is 60 s): a call may
/// wait for a load, the user's answer to the access prompt and a cold start.
const CODEX_TOOL_TIMEOUT_SECS: u64 = 300;
/// The executable the MCP server entry runs, in the environment so the
/// launch adapters can name it.
const ENV_EXECUTABLE: &str = "MONOCODE_BROWSER_EXECUTABLE";
/// Its database file in the app's data directory.
const DATABASE: &str = "browser.sqlite3";
/// Where Windows copies of the executable go, so a session holding
/// `browser-mcp` open never locks the app against an update.
const COPIES: &str = "bin-native";

static ENDPOINT: OnceLock<String> = OnceLock::new();
static EMBEDDER: OnceLock<Arc<NativeEmbedder>> = OnceLock::new();
static DATA_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// The app as the browser module sees it.
struct NativeEmbedder {
    db: Mutex<Connection>,
    scale: Mutex<Option<f64>>,
    listeners: Mutex<Vec<async_channel::Sender<BrowserState>>>,
}

impl Embedder for NativeEmbedder {
    fn with_db(&self, f: &mut dyn FnMut(&Connection)) {
        let conn = self.db.lock().unwrap_or_else(|e| e.into_inner());
        f(&conn);
    }

    fn state_changed(&self, state: &BrowserState) {
        let mut listeners = self.listeners.lock().unwrap_or_else(|e| e.into_inner());
        listeners.retain(|listener| listener.try_send(state.clone()).is_ok());
    }

    fn display_scale(&self) -> Option<f64> {
        *self.scale.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Start the browser module: its database (in `data_dir`, in memory
/// without one), its host and its loopback endpoint. Once per process;
/// later calls do nothing.
pub fn start(data_dir: Option<&Path>) -> Result<(), String> {
    if EMBEDDER.get().is_some() {
        return Ok(());
    }
    let db = match data_dir {
        Some(dir) => {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            Connection::open(dir.join(DATABASE))
        }
        None => Connection::open_in_memory(),
    }
    .map_err(|e| e.to_string())?;
    let embedder = Arc::new(NativeEmbedder {
        db: Mutex::new(db),
        scale: Mutex::new(None),
        listeners: Mutex::new(Vec::new()),
    });
    if EMBEDDER.set(embedder.clone()).is_err() {
        return Ok(());
    }
    let _ = DATA_DIR.set(data_dir.map(Path::to_path_buf));
    monocode_browser::init(embedder, data_dir)?;
    let host = monocode_browser::host().ok_or("Browser module did not start")?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let endpoint = listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .to_string();
    let _ = ENDPOINT.set(endpoint);
    std::thread::Builder::new()
        .name("browser-endpoint".into())
        .spawn(move || monocode_browser::transport::serve(listener, host))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// App exit: the MonoCode browser goes with the app.
pub fn shutdown() {
    monocode_browser::shutdown();
}

/// The running host, once [`start`] ran.
pub fn host() -> Option<Arc<BrowserHost>> {
    monocode_browser::host()
}

/// The state now, once [`start`] ran.
pub fn state() -> Option<BrowserState> {
    host().map(|host| host.state())
}

/// Every state change from now on. The UI keeps only the newest.
pub fn subscribe() -> async_channel::Receiver<BrowserState> {
    let (send, receive) = async_channel::unbounded();
    if let Some(embedder) = EMBEDDER.get() {
        embedder
            .listeners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(send);
    }
    receive
}

/// The pixel ratio of the display the main window is on; the MonoCode
/// browser starts at it.
pub fn set_display_scale(scale: f64) {
    if let Some(embedder) = EMBEDDER.get() {
        *embedder.scale.lock().unwrap_or_else(|e| e.into_inner()) =
            (scale.is_finite() && scale > 0.0).then_some(scale);
    }
}

/// The key the browser module knows a session by.
pub fn session_key(session: i64) -> String {
    session.to_string()
}

/// How the agent of `kind` reaches the browser in Native.
pub fn tools(kind: Kind) -> Tools<'static> {
    match kind {
        Kind::Claude | Kind::Codex => Tools::Mcp,
        // A per-session `opencode serve` and Pi's own shell inherit the
        // environment; neither takes MCP servers at launch.
        Kind::OpenCode => Tools::Cli,
        kind if kind.is_pi() => Tools::Cli,
        // ACP agents get the server in `session/new`.
        _ => Tools::Mcp,
    }
}

/// What a turn sends to the agent about the browser. The turn is active
/// for the browser until this is dropped: keep it until the provider's
/// run returns.
#[derive(Debug, Default)]
pub struct Turn {
    session: Option<i64>,
    /// Goes into the provider's environment: endpoint, token, executable.
    pub environment: HashMap<String, String>,
    /// The `<monocode_browser>` block for the prompt.
    pub instructions: Option<String>,
}

impl Drop for Turn {
    fn drop(&mut self) {
        if let Some(session) = self.session {
            // Actions still running end as `unknown`.
            monocode_browser::turn_finished(&session_key(session));
        }
    }
}

/// A turn is about to start in `session`. `eligible`: a turn the user
/// sends from a regular thread, not one an agent sent through the app or
/// an orchestration run; those use the browser only with a grant the
/// thread already holds. `full_access`: the thread runs with full access,
/// which grants like "Always on". The result is empty when the turn
/// carries no browser.
pub fn begin_turn(session: i64, cwd: &Path, kind: Kind, eligible: bool, full_access: bool) -> Turn {
    let Some(host) = host() else {
        return Turn::default();
    };
    let key = session_key(session);
    let access = turn::turn_access(&host.state(), &key, eligible, full_access);
    if access == Some(TurnAccess::Grant) {
        host.set_grant(&key, WINDOW, true);
    }
    host.turn_started(&key, WINDOW, &cwd.to_string_lossy(), None);
    let mut turn = Turn {
        session: Some(session),
        environment: HashMap::new(),
        instructions: None,
    };
    let (Some(access), Some(token), Some(endpoint)) =
        (access, host.token_for(&key), ENDPOINT.get().cloned())
    else {
        return turn;
    };
    let executable = match executable() {
        Ok(executable) => executable,
        Err(error) => {
            eprintln!("browser: no executable for the agent: {error}");
            return turn;
        }
    };
    let app = devservers::agent_note(cwd, &devservers::for_dir(cwd));
    let block = register::instructions_block(tools(kind), Some(&app));
    turn.instructions = Some(if access == TurnAccess::Ask {
        turn::with_ask_note(&block)
    } else {
        block
    });
    turn.environment = HashMap::from([
        (ENV_ENDPOINT.to_string(), endpoint),
        (ENV_TOKEN.to_string(), token),
        (ENV_EXECUTABLE.to_string(), executable),
    ]);
    turn
}

/// A Mono (`mono_session`) starts `session` or hands it work: the session
/// takes the Mono's grant, unless the user turned it off there.
pub fn inherit_mono_grant(mono_session: i64, session: i64) {
    let Some(host) = host() else { return };
    let (mono, key) = (session_key(mono_session), session_key(session));
    if turn::inherits_mono_grant(&host.state(), &mono, &key) {
        host.set_grant(&key, WINDOW, true);
    }
}

/// The session was deleted: its grant goes, its tabs are closed or given
/// back.
pub fn session_deleted(session: i64) {
    monocode_browser::session_deleted(&session_key(session));
}

/// The executable the MCP server entry names: the app itself, or on
/// Windows a per-build copy of it.
fn executable() -> Result<String, String> {
    let source = server_binary::source_executable()?;
    let dir = DATA_DIR
        .get()
        .cloned()
        .flatten()
        .unwrap_or_else(std::env::temp_dir)
        .join(COPIES);
    let path = server_binary::install(&source, &dir, server_binary::uses_copy())?;
    Ok(path.to_string_lossy().into_owned())
}

/// The turn's environment carries the browser.
pub fn configured(environment: &HashMap<String, String>) -> bool {
    [ENV_ENDPOINT, ENV_TOKEN, ENV_EXECUTABLE]
        .iter()
        .all(|key| environment.contains_key(*key))
}

/// The stdio server definition, for Claude's `--mcp-config`.
pub fn mcp_server(environment: &HashMap<String, String>) -> Option<Value> {
    configured(environment).then(|| {
        json!({
            "type": "stdio",
            "command": environment[ENV_EXECUTABLE],
            "args": [MCP_SUBCOMMAND],
        })
    })
}

/// The server for ACP's `session/new` and `session/load`.
pub fn acp_server(environment: &HashMap<String, String>) -> Option<Value> {
    let mut server = mcp_server(environment)?;
    // ACP's stdio server shape omits the transport discriminator.
    server.as_object_mut()?.remove("type");
    server["name"] = SERVER_NAME.into();
    server["env"] = [ENV_ENDPOINT, ENV_TOKEN]
        .into_iter()
        .map(|key| json!({"name": key, "value": environment[key]}))
        .collect::<Vec<_>>()
        .into();
    Some(server)
}

/// Codex filters its MCP children's environment: the entry lists the two
/// variables to pass through, and allows long calls.
pub fn configure_codex(command: &mut Command, environment: &HashMap<String, String>) {
    if !configured(environment) {
        return;
    }
    for (key, value) in [
        ("command", json!(environment[ENV_EXECUTABLE])),
        ("args", json!([MCP_SUBCOMMAND])),
        ("env_vars", json!([ENV_ENDPOINT, ENV_TOKEN])),
        ("tool_timeout_sec", json!(CODEX_TOOL_TIMEOUT_SECS)),
    ] {
        // JSON's strings, numbers and arrays are valid TOML literals here.
        command.args(["-c", &format!("mcp_servers.{SERVER_NAME}.{key}={value}")]);
    }
}

/// `monocode browser …` and `monocode browser-mcp`, when `args` (without
/// the program name) asks for one: the exit code to end with.
pub fn run_subcommand(args: &[String]) -> Option<i32> {
    match args.first().map(String::as_str) {
        Some(MCP_SUBCOMMAND) => Some(monocode_browser::agent::mcp::run()),
        Some("browser") => Some(monocode_browser::agent::cli::run(args[1..].to_vec())),
        _ => None,
    }
}

/// One picture of a tab shown live, decoded for drawing.
pub struct LivePicture {
    /// Counts the pictures of one view, from 1.
    pub seq: u64,
    /// The page's viewport in CSS pixels: input coordinates are in this
    /// space, whatever the picture's own size.
    pub width: f64,
    pub height: f64,
    /// The picture, BGRA as GPUI draws it.
    pub pixels: image::RgbaImage,
}

/// Decode a picture [`BrowserHost::live_take`] handed over
/// ([`live::LiveFrame::packed`]).
pub fn unpack_live(packed: &[u8]) -> Result<LivePicture, String> {
    if packed.len() <= live::PACKED_HEADER {
        return Err("The picture is empty.".into());
    }
    let word = |at: usize| -> [u8; 8] { packed[at..at + 8].try_into().unwrap_or_default() };
    let seq = u32::from_le_bytes(packed[4..8].try_into().unwrap_or_default());
    let format = if packed[0] == 1 {
        image::ImageFormat::Png
    } else {
        image::ImageFormat::Jpeg
    };
    let mut pixels = image::load_from_memory_with_format(&packed[live::PACKED_HEADER..], format)
        .map_err(|error| error.to_string())?
        .into_rgba8();
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok(LivePicture {
        seq: u64::from(seq),
        width: f64::from_le_bytes(word(8)),
        height: f64::from_le_bytes(word(16)),
        pixels,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_live_picture_unpacks_to_bgra_with_its_viewport() {
        use base64::Engine as _;
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let frame = live::LiveFrame {
            tab: "t1".into(),
            data: base64::engine::general_purpose::STANDARD.encode(&png),
            format: "png",
            width: 1.5,
            height: 1.0,
            seq: 7,
        };
        let picture = unpack_live(&frame.packed().unwrap()).unwrap();
        assert_eq!(picture.seq, 7);
        assert_eq!((picture.width, picture.height), (1.5, 1.0));
        assert_eq!(picture.pixels.dimensions(), (3, 2));
        assert_eq!(picture.pixels.get_pixel(0, 0).0, [30, 20, 10, 255]);
        assert!(unpack_live(&[0; 4]).is_err());
    }

    use super::*;

    fn environment() -> HashMap<String, String> {
        HashMap::from([
            (ENV_ENDPOINT.to_string(), "127.0.0.1:9".to_string()),
            (ENV_TOKEN.to_string(), "secret".to_string()),
            (ENV_EXECUTABLE.to_string(), "/apps/monocode".to_string()),
        ])
    }

    #[test]
    fn launches_get_the_server_only_with_the_credentials() {
        assert!(mcp_server(&HashMap::new()).is_none());
        assert_eq!(
            mcp_server(&environment()).unwrap(),
            json!({"type": "stdio", "command": "/apps/monocode", "args": ["browser-mcp"]})
        );
        let acp = acp_server(&environment()).unwrap();
        assert_eq!(acp["name"], SERVER_NAME);
        assert!(acp.get("type").is_none());
        assert_eq!(acp["env"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn codex_passes_the_credentials_through_and_waits_long_enough() {
        let mut command = Command::new("codex");
        configure_codex(&mut command, &environment());
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&format!(
            "mcp_servers.{SERVER_NAME}.env_vars=[\"{ENV_ENDPOINT}\",\"{ENV_TOKEN}\"]"
        )));
        assert!(args.contains(&format!(
            "mcp_servers.{SERVER_NAME}.tool_timeout_sec={CODEX_TOOL_TIMEOUT_SECS}"
        )));
        let mut bare = Command::new("codex");
        configure_codex(&mut bare, &HashMap::new());
        assert_eq!(bare.get_args().count(), 0);
    }

    #[test]
    fn shell_agents_use_the_cli_and_the_others_mcp() {
        assert_eq!(tools(Kind::Claude), Tools::Mcp);
        assert_eq!(tools(Kind::Cursor), Tools::Mcp);
        assert_eq!(tools(Kind::OpenCode), Tools::Cli);
        assert_eq!(tools(Kind::Pi), Tools::Cli);
    }
}
