Restart note for the wiring work (boot, adapters, workspace area, pages).
Delete before committing.

Layout
- Cargo.toml: every engine package but remote; view-files, view-scm,
  view-pages, view-inbox, view-settings, view-workbench, monocode-host.
- slots.rs: AppSlots, install(), window_workspace(), cached_view().
- panes/: workspace area (mod.rs), standalone.rs (--view page-*, tab-*,
  workspace for screenshots), explorer.rs, changes.rs, inbox_tab.rs.
- pages/: search, notes, automations, inbox, settings; mod.rs dispatches.
- adapters/: files, scm, automations, notes, search, mcp, skills,
  projects_data, inbox, settings.
- main.rs: view crate inits, slots::install, screenshot runs never focus.

Helpers (subagents)
- A: files + scm + explorer + changes (adapters/files.rs, adapters/scm.rs,
  panes/explorer.rs, panes/changes.rs).
- B: pages (adapters automations, notes, search, mcp, skills,
  projects_data; pages search, notes, automations).
- C: inbox (adapters/inbox.rs, pages/inbox.rs, panes/inbox_tab.rs).
- D: settings (adapters/settings.rs, pages/settings.rs, glass.rs).

Me
- [ ] boot: projects, history, inbox, automations (start), orchestration,
      control server into registry and children.
- [ ] hooks between packages per /tmp/mc/needs-*.md.
- [ ] workspace area: PaneTree, SurfaceTabs, TabGroupMenu, leaves.
- [ ] main_pane.rs draws the workspace slot.
- [ ] `monocode-app host ...`.
- [ ] verification: screenshots, live test, control e2e, clippy, tests.
