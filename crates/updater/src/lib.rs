//! Self-update for the native app over the release feed and signing key the
//! Tauri app used. Replaces tauri-plugin-updater, tauri-plugin-process's
//! `relaunch`, and src/app/model/updater.ts, plus the release notes models.
//!
//! # Pieces
//!
//! - [`Updater`] and [`Update`]: check `latest.json`, download with progress,
//!   verify the minisign signature, and install. Install replaces the `.app`
//!   on macOS, starts the NSIS installer on Windows, and swaps the AppImage
//!   (or runs dpkg or rpm) on Linux, as tauri-plugin-updater did. The calls
//!   block; the `*_async` versions run on a worker thread under any executor.
//! - [`relaunch()`]: start the app again once this process exits.
//! - [`flow::UpdaterFlow`]: the port of updater.ts. The app implements
//!   [`flow::UpdaterHost`] for dialogs, the update sound, the installed marker,
//!   and relaunch.
//! - [`update_notice`], [`release_notes`], [`release_notes_workspace`]: the
//!   "What's new" models.
//!
//! # Wiring it into the app
//!
//! The feed URL and public key are baked in at build time from
//! `TAURI_UPDATER_ENDPOINT` and `TAURI_UPDATER_PUBKEY`. Without them, checks
//! report "not configured", like a Tauri build without the release config.
//!
//! ```no_run
//! use monocode_updater::flow::{DialogOptions, UpdaterFlow, UpdaterHost};
//! use monocode_updater::{DownloadEvent, Result, Update, Updater, UpdaterConfig};
//!
//! struct AppHost;
//!
//! impl UpdaterHost for AppHost {
//!     fn app_version(&self) -> String {
//!         env!("CARGO_PKG_VERSION").to_string()
//!     }
//!     async fn check(&self) -> Result<Option<Update>> {
//!         Updater::new(UpdaterConfig::from_build_env(self.app_version()))?
//!             .check_async()
//!             .await
//!     }
//!     async fn download_and_install(
//!         &self,
//!         update: &Update,
//!         on_event: &mut dyn FnMut(DownloadEvent),
//!     ) -> Result<()> {
//!         update.download_and_install_async(on_event).await
//!     }
//!     async fn message(&self, _text: &str, _options: DialogOptions) {
//!         // window.prompt(PromptLevel::Info, ...) in GPUI
//!     }
//!     async fn ask(&self, _text: &str, _options: DialogOptions) -> bool {
//!         false
//!     }
//!     fn announce_update_available(&self, _version: &str) {}
//!     fn remember_installed_update(&self, version: &str) {
//!         # let kv = monocode_settings::Kv::in_memory();
//!         monocode_updater::update_notice::remember_installed_update(version, &kv);
//!     }
//!     async fn relaunch(&self) -> Result<()> {
//!         monocode_updater::relaunch()?;
//!         // then cx.quit()
//!         Ok(())
//!     }
//! }
//!
//! # async fn run() {
//! let flow = UpdaterFlow::new(AppHost);
//! let snapshot = flow.run_update_flow(true, |snapshot| println!("{snapshot:?}")).await;
//! # }
//! ```

mod background;
pub mod config;
mod error;
pub mod flow;
mod install;
pub mod manifest;
mod relaunch;
pub mod release_notes;
pub mod release_notes_workspace;
#[cfg(any(test, feature = "sign"))]
pub mod sign;
pub mod update_notice;
mod updater;
mod verify;

#[cfg(test)]
mod local_feed_tests;

pub use config::{BUILD_ENDPOINT, BUILD_PUBKEY, Installer, UpdaterConfig};
pub use error::{Error, Result};
pub use relaunch::relaunch;
pub use updater::{DownloadEvent, Update, Updater};
pub use verify::verify_signature;
