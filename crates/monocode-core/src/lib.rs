//! UI-independent services shared by the native migration and Tauri.
pub mod accounts;
pub mod agent;
pub mod app_access;
pub mod app_update;
pub mod automations;
pub mod catalog;
pub mod checkpoint;
pub mod context;
pub mod desktop_preferences;
pub mod diff;
pub mod external_editor;
pub mod files;
pub mod follow_up;
pub mod git;
pub mod harness_updates;
pub mod inbox;
pub mod mobile;
pub mod mono;
pub mod notes;
pub mod process;
pub mod project_appearance;
pub mod provider;
pub mod remote;
pub mod resources;
pub mod response;
pub mod scm;
pub mod session_filters;
pub mod session_groups;
pub mod session_title;
pub mod shell_run;
pub mod store;
pub mod time;
pub mod tool;
pub mod transcript;
pub mod usage;
pub mod user_links;
pub mod visual;
pub mod visual_bridge;

pub mod composer;
pub mod contributions;
pub mod orchestration;
pub mod skills;

pub mod search;

pub mod cli;
pub mod mcp;

pub mod worktree_management;
