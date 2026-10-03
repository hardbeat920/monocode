//! The MCP settings section (McpSettings.tsx), which the settings page
//! embeds.

pub mod add_form;
pub mod data;
pub mod picker;
pub mod view;

#[cfg(test)]
mod tests;

pub use add_form::AddServerForm;
pub use data::{
    LocalMcp, McpCall, McpConnection, McpData, McpProvider, McpScope, McpServerRow,
    McpSettingsSnapshot,
};
pub use picker::{McpPicker, McpPickerOption};
pub use view::{McpFilter, McpSettingsView};
