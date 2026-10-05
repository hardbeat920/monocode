# macOS panel qualification

The default `panel_and_hotkey` integration test passed all five checks. It verified the quick composer panel class, key and main policy, nonactivating style, window level, collection behavior, vibrancy, idempotent setup, ordinary-window rejection, top-edge-preserving height changes, and Carbon hotkey registration and release. The windows stayed hidden. This result does not prove delivery of a shortcut pressed in another app.

The explicit `--ignored` presentation mode remains unverified. Its first run failed while waiting for a visible panel to become key. The [first log](macos-panel-presentation-first.log) records that failure. A standalone test initially lacked GPUI's application launch callback and AppKit loop. Follow-up fixture runs added `finishLaunching`, an AppKit main-queue callback, and `NSApplication::run`. The [running-loop log](macos-panel-running-loop.log) and [regular-policy log](macos-panel-regular-loop.log) still reported a visible panel with `key=false`, `main=false`, and `active=false`. The latter used GPUI's regular application activation policy. The product helper did not change.

A read-only console check then found that the host was locked. The [filtered session evidence](macos-panel-console-state.json) reports `CGSSessionScreenIsLocked=true` and an on-console user. The foreground process was loginwindow. This explains why these runs cannot qualify focus transfer in an unlocked desktop session. They do not prove a defect in `present_ns`. The retained Tauri helper also calls `orderFrontRegardless` followed by `makeKeyWindow` for its promoted panel.

The failed presentation runs prove that the owned panel became visible, remained non-main, and did not activate its application or replace the foreground process. They did not reach the subsequent hide, show, draft retention, or ordinary-window fallback assertions. Those checks require an unlocked console. No unlocking, keyboard input, permission prompt acceptance, preference change, or service change occurred. The fixture closed its owned windows when its assertion failed.

The separate GPUI quick composer regression covers cached view behavior. It preserves draft text and an attachment through dismissal and a second show, refreshes the available project and current directory, and restores prompt focus. It does not qualify AppKit key status or physical global shortcut delivery.

Screen capture permission, capture interaction, real shortcut delivery, the ordinary-window presentation fallback, and actual cross-app focus restoration remain open.
