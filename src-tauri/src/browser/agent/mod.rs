//! Work package 3: how agents reach the browser module (spec 12).
//!
//! Two thin clients in the `monocode` executable, sharing one transport:
//!
//! - [`mcp`]: `monocode browser-mcp`, a stdio MCP server exposing the
//!   `browser_*` tools, for providers MonoCode can register MCP servers with.
//! - [`cli`]: `monocode browser ACTION ...`, for every other provider (its
//!   instructions are injected into the turn) and for manual testing.
//! - [`client`]: one JSON line to the control endpoint from
//!   `MONOCODE_BROWSER_ENDPOINT` with `MONOCODE_BROWSER_TOKEN`, one
//!   [`ActionResult`](super::protocol::ActionResult) back.
//! - [`tools`]: tool names, schemas and descriptions; the descriptions come
//!   from `docs/browser-instructions.md`.
//! - [`register`]: Tauri commands that register the MCP server with a
//!   provider and give the UI the text to inject.
//! - [`server_binary`]: which executable that registration names (a per-build
//!   copy on Windows, so a running server never locks `MonoCode.exe`).
//!
//! Neither client keeps state, chooses a session or sees a CDP id: identity is
//! the token the provider process inherited, and every decision is the host's.

pub mod cli;
pub mod client;
pub mod mcp;
pub mod register;
pub mod server_binary;
pub mod tools;
