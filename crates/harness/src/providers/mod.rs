//! One module per provider CLI.

#[cfg(feature = "antigravity")]
pub mod antigravity;
#[cfg(feature = "claude")]
pub mod claude;
#[cfg(feature = "codex")]
pub mod codex;
#[cfg(feature = "cursor")]
pub mod cursor;
#[cfg(feature = "droid")]
pub mod droid;
#[cfg(feature = "fx")]
pub mod fx;
#[cfg(feature = "grok")]
pub mod grok;
#[cfg(feature = "hermes")]
pub mod hermes;
#[cfg(feature = "opencode")]
pub mod opencode;
#[cfg(feature = "pi")]
pub mod pi;
