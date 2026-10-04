Restart note for the settings slice (adapters/settings.rs, pages/settings.rs,
glass.rs). Delete before committing.

Done
- Read the view crate (host.rs, page.rs, native_glass.rs), the engine APIs
  each host adapts, and App.tsx's SettingsView wiring.

In progress
- adapters/settings.rs: every host over the engine and services.

Next
- pages/settings.rs: the page, cached per window, section from Kv
  (`monocode.settingsSection`), props from History and Projects.
- glass.rs: NativeGlass per window, Theme observer.
- Screenshots: default, providers, appearance, mcp (MONOCODE_SETTINGS_SECTION).
