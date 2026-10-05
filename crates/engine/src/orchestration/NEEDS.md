# What orchestration needs from other packages

Orchestration works around each gap locally. Calls into the window and
workspace go through `OrchestrationPeers` (peers.rs), which has defaults.

## Shared file I changed

1. crates/engine/Cargo.toml: `orchestration` now turns on `submit`,
   `attention`, `history`, and `projects`. Managed worker turns and Stop go
   through `Submit`; pending worker input reads attention's
   `pending_approval_for_session`; the app API reads history's session folders
   and notes; worker checkouts use projects' worktree actions.

## Harness crate

2. `providers/opencode/deps.rs::extract_tool_preview` is a stub that returns
   `None`, so OpenCode tool events carry no preview. The orchestrator's scope
   check reads the write preview, so it never checks an OpenCode worker's
   writes. It should call `monocode_core::reducer::preview::extract_tool_preview`,
   which exists. The scratch-write test builds the preview with the core
   function until then.

## Submit

3. Resolved. `SubmitOrchestrationHooks::discover_settings` returns a result.
   Discovery errors stop the planning turn before the lead provider runs.
   The failed proposal retains the original error message.

## Workspace

4. `consolidate_orchestration_tabs` (workspace.rs) is the App.tsx effect over
   `tabs`, `activeTabId`, and runs. The workspace package should run it when
   either changes, with `Orchestration::orchestrator(cx).read(cx).snapshot()`.
5. `queueWorkerPanes` and `onOpenWorkerDetails` (App.tsx 9224-9291): use
   `prepare_orchestration_worker_details`, then open one agent tab per worker
   in the pane beside the lead (`newAgentTab` and `openEditorTab`).
6. `OrchestrationPeers::launch_session`: `launchQuickSession` for the window
   behind a control owner. `AppLaunch` holds the fields `QuickLaunch` had;
   automations' `QuickLaunchRequest` has the same shape. The default refuses,
   so `sessions.start` fails until the app fills it in.
7. `OrchestrationPeers::check_open_worktree_files`: `checkOpenWorktreeFiles`
   before an orchestration worktree is removed.

## Attention

8. `ApprovalRouter::orchestration_lead_for` should return
   `Orchestration::orchestrator(cx).read(cx).for_session(id)` lead id, so an
   approval for a worker opens its lead (`onOpenApprovalSession`).

## History

9. Session deletion should go through `Orchestration::delete_session(id,
   remove)`, which stops the run and drains control writes before `remove`
   deletes rows, then reloads a lead's pruned run. History's host methods for
   the orchestrator are no-ops today.
10. `history_with_live_sessions` takes `LiveRun`s; build them with
    `summary::live_run` over `Orchestrator::snapshot`.

## App wiring

11. Start the control server with `start_control_server(cx)` and pass the
    host to `OrchestrationConfig::native`, to the registry's
    `RegistryOptions::turn_control` (`ControlTurns { host, owner }`), and to
    `configure_child` for harness spawns. Call `control::owner_closed` when a
    window closes. The engine holds every window's sessions, so one owner id
    for the app is enough; the TypeScript routed by window label.
12. `OrchestrationPeers::probe_availability`: `probeHarnessAvailability` before
    worker models are discovered.

## Receipt compatibility

13. Saved control receipts keep their original signature bytes. Retries first
    compare exact strings, then compare equal JSON values across object key
    order and decimal number spelling. Raw number digits prevent floating-point
    rounding from hiding changed input. Pending requests still compare exact
    signatures. GPUI enables `serde_json/preserve_order` in the current graph,
    so the original key-sorting difference did not reproduce there. The
    hydrated approval fixture did reproduce TypeScript's `7` signature
    rejecting unchanged native input `7.0` before this comparison fix.
