# Native preview 13 change audit

Prepared on 2026-11-03 for the next native preview. The release-facing summary is
in the [preview 13 release notes](https://github.com/hardbeat920/monocode/blob/native-v0.16.0-preview.13/docs/native/preview-release-notes.md).

## Scope and baseline

- Last published native preview: [0.16.0-preview.12](https://github.com/hardbeat920/monocode/releases/tag/native-v0.16.0-preview.12), published 2026-10-31 at 18:32:45 UTC. GitHub release metadata was checked while preparing these notes.
- Baseline tag: `native-v0.16.0-preview.12`, resolving to `3fa189d1abf7423b68a979b1c8f4e6874d52f843`.
- Reviewed through: `4550fdedce9344e41ce5836b2c7e16c1cdcbd471` on `main` (2026-11-03).
- [Exact source comparison](https://github.com/hardbeat920/monocode/compare/3fa189d1abf7423b68a979b1c8f4e6874d52f843...4550fdedce9344e41ce5836b2c7e16c1cdcbd471).

The inventory uses Git ancestry, rather than a date filter. Mobile and macOS work
authored before preview 12 but merged afterward is included. The summary describes
the final diff: for example, mouse-wheel glide remains, but the elastic row stretch
introduced in the same period was removed. Vendoring the macOS renderer does not
mean every file in that initial vendor import contains a new feature.

Release preparation also fixes the changes-shelf test initializer to satisfy strict
Clippy. This changes only test setup, with no runtime behavior change. The native
release workflow derives `0.16.0-preview.13` from its tag; the production app's
version remains `0.16.0`.

## Desktop and companion scope

The release notes cover the native desktop changes and the paired-phone protocol
changes it hosts. Changes under `mobile/`, including the Monos-only layout, iOS
animations, Android controls, adaptive sidebar, and hinge-driven composer focus,
require a separately distributed companion update.

The mobile build profiles now pin the Xcode 27.1 image; the iOS source build requires
Xcode 27.1 and its SDK for hinge observation. Native transcript and hinge-observer
changes require a new native companion build rather than only an EAS Update.
See the [mobile source guide](../../mobile/README.md).

## Shared React/Tauri changes in the same range

These changes belong in the repository inventory but should not be presented as
new fixes in the Rust/GPUI desktop binary:

- **Codex Mono storage sync** keeps `session_index.jsonl.lock` private to each Mono instead of linking it to the account's lock. It removes stale symlinks on Unix and stale hard links on Windows, checks file identity and deletes through the same Windows handle with the required DELETE access, serializes concurrent preparations, and tolerates a link already removed by another preparation. Regression tests cover lock independence and concurrent preparation. This implementation is in `src-tauri/src/codex_mono_store.rs`, from [#1297](https://github.com/hardbeat920/monocode/pull/1297).
- **Usage-limit recovery from mobile** is wired into the React/Tauri host as well as the native host. The web host validates session tokens and concurrent actions, uses the desktop's continuation path, updates watches when usage state changes without transcript changes, and restores a manual retry notice when submission fails. Its automatic-resume preference lasts only until the session closes or the app restarts; the native host persists its preference. From [#1208](https://github.com/hardbeat920/monocode/pull/1208).
- **Plain-text Markdown fences** receive the prose-highlighting fix in the web renderer as well as GPUI. From [#1263](https://github.com/hardbeat920/monocode/pull/1263).
- **Video metadata loading** is preserved in the React file viewer by avoiding redundant `pause()` calls on an already paused player, including effect cleanup replay. This prevents interrupting in-progress metadata loading. From `5a31951a`.
- **Mono chat pacing instructions** in the web app now request a short reply before tools and background sessions for longer work; the prompt version advances to 19. The native MCP app tool and native system-instruction changes are separate parts of this period's Mono work. From `5501f8b8`.

## Merged contributions

- [#1174](https://github.com/hardbeat920/monocode/pull/1174), @nwoolls: adaptive mobile sidebar, iPhone Duo posture focus, and keyboard transition fix.
- [#1208](https://github.com/hardbeat920/monocode/pull/1208), @nwoolls: mobile continuation of usage-limited sessions in both desktop hosts.
- [#1221](https://github.com/hardbeat920/monocode/pull/1221), @hardbeat920: recovery from stalled macOS display links and live-resize coordination.
- [#1251](https://github.com/hardbeat920/monocode/pull/1251), @keyCat: dismissing the native update notice without opening What's new.
- [#1252](https://github.com/hardbeat920/monocode/pull/1252), @visualfusion: retry provider startup on `ETXTBSY` and flush executable test fixtures.
- [#1254](https://github.com/hardbeat920/monocode/pull/1254), @Juns-g: favorite stars in native model pickers.
- [#1256](https://github.com/hardbeat920/monocode/pull/1256), @Juns-g: consistent provider/model choice for new native chats.
- [#1258](https://github.com/hardbeat920/monocode/pull/1258), @Juns-g: refresh titles of open remote chats when the host changes them.
- [#1263](https://github.com/hardbeat920/monocode/pull/1263), @nwoolls: plain-text Markdown fence highlighting in native and web renderers.
- [#1265](https://github.com/hardbeat920/monocode/pull/1265), @nwoolls: editing context menus for native inputs and terminals.
- [#1268](https://github.com/hardbeat920/monocode/pull/1268), @knot2006: Windows terminal shell discovery and PowerShell preference.
- [#1269](https://github.com/hardbeat920/monocode/pull/1269), @404khai: Android menus, controls, and Mono profile sheet fixes.
- [#1271](https://github.com/hardbeat920/monocode/pull/1271), @Juns-g: preserve reading position when transcript rows fold away.
- [#1272](https://github.com/hardbeat920/monocode/pull/1272), @Juns-g: restore the reading position when an unloaded transcript reopens.
- [#1297](https://github.com/hardbeat920/monocode/pull/1297), @oskar-albers: Codex Mono storage lock isolation and concurrent-preparation fixes in Tauri.

## Verification boundary

The notes were checked against commit history and the final source diff. Added or
modified tests are documented as coverage changes; release-check results are
recorded separately in [verification](verification.md). Existing platform and
packaging limitations in that document and [parity](parity.md) still apply.

Documentation checks verified whitespace, all relative Markdown links, exact
coverage of the 71 non-merge commits and 15 merged pull requests, preview numbering,
and preservation of the existing installation/testing guidance apart from making
its links resolve to the preview 13 tag on GitHub.

The mobile source guide specifically leaves native transcript scroll anchoring
after adaptive resizing for simulator/device validation; mocked React tests
establish component identity, not native scroll offsets. The wider sidebar and
posture support should not be read as a claim of completed device testing.

## Complete non-merge commit inventory

The following list records every non-merge commit in the reviewed range, in
`git log --reverse` order (oldest first). Merge-only integration commits are omitted;
their feature commits are included, and merged pull requests are linked above.

- [9d438970](https://github.com/hardbeat920/monocode/commit/9d4389706db5b33a3c15b6a11c0a6b216763a54d) — Add adaptive mobile sidebar and iPhone Duo support
- [110b9d66](https://github.com/hardbeat920/monocode/commit/110b9d66967aee0fb38ae0aed78a473f28084c58) — Reset settled keyboard height at each transition start
- [5e188b47](https://github.com/hardbeat920/monocode/commit/5e188b47d6d70bc1a28e2dbe500333fd7565c6fc) — Support continuing usage-limited sessions from mobile
- [ba01b0e3](https://github.com/hardbeat920/monocode/commit/ba01b0e39217f6185421c23df28c1fad69b4529a) — Keep the update card's dismiss button from opening What's new
- [e96c9266](https://github.com/hardbeat920/monocode/commit/e96c926689016986b3580b4c0316541a8eacda36) — fix(core): retry provider spawns on ETXTBSY and flush test scripts
- [2a664081](https://github.com/hardbeat920/monocode/commit/2a664081ae0cd8703f1242e4d7d0f9afb9be43ed) — fix(native): add a favorite star to model picker rows
- [72036836](https://github.com/hardbeat920/monocode/commit/720368361181a512514930af9f92d172f1055e85) — fix(native): resolve the new-chat provider and model in one place
- [071f62b2](https://github.com/hardbeat920/monocode/commit/071f62b27b29bcd68f345cbd791f1b4f6783b4c7) — fix(native): apply host-changed titles to open remote chats
- [7a2bd910](https://github.com/hardbeat920/monocode/commit/7a2bd9102c37515c6c5b0007f546f2ab4833764f) — fix(native): keep remote providers and explicit-provider models on mobile
- [06dd528c](https://github.com/hardbeat920/monocode/commit/06dd528c3c777ef143010b108731e287b5ccd92e) — Fix prose fence highlighting in native and web renderers
- [395be560](https://github.com/hardbeat920/monocode/commit/395be56078424225905d1c32c17077d67e017397) — Add editing context menus to native inputs and terminals
- [3fa32d4d](https://github.com/hardbeat920/monocode/commit/3fa32d4dec216dcdf8bc2d8aab3e429f63efa8b2) — Fix native terminal shell selection on Windows
- [48d8583e](https://github.com/hardbeat920/monocode/commit/48d8583e08ed07075a4306c57f6a70b96610feed) — Prefer PowerShell for native Windows terminals
- [821bafb5](https://github.com/hardbeat920/monocode/commit/821bafb55c12827b9d0348aee76c83768fcfba2e) — Resolve Windows terminal shells using the child PATH
- [414ca6cc](https://github.com/hardbeat920/monocode/commit/414ca6cc280f7e88f2f48339511aaa4575c38412) — Polish Android menus and the Mono profile sheet
- [5c4d582e](https://github.com/hardbeat920/monocode/commit/5c4d582eef331f8be8c78012d3889af90252e882) — fix(native): keep the reader in their turn when the row they read folds away
- [bf1efdd3](https://github.com/hardbeat920/monocode/commit/bf1efdd31dd90462cebf2f23e31090ffbf44db0d) — fix(native): restore the reading position when an evicted transcript is reinstalled
- [f184d62a](https://github.com/hardbeat920/monocode/commit/f184d62aefe3f566741edbaacc69f7612c85e390) — fix(native): compare the full row key when the reader's row is gone
- [c264992e](https://github.com/hardbeat920/monocode/commit/c264992efaf362fcd7fefd1fe2a343f10cfdc371) — fix(native): use the configured model for any explicit phone provider and project-hidden providers
- [6b09e2cf](https://github.com/hardbeat920/monocode/commit/6b09e2cf1423c10336ff55aef1f4c31937d5dddb) — Resolve terminal module declaration conflict with main
- [9c15116e](https://github.com/hardbeat920/monocode/commit/9c15116ee8a81c75ad1a2e741e07041f503eedea) — Vendor gpui-pre-macos 0.3.9 unchanged
- [189d64a0](https://github.com/hardbeat920/monocode/commit/189d64a05164240e7f2804283c4b73d4145e9b6e) — Keep macOS windows repainting after their display link stalls
- [c341daf6](https://github.com/hardbeat920/monocode/commit/c341daf64dbe63c5d84e29df6628436fffe4242e) — Let live-resize redraws own the window layer on macOS
- [be06a810](https://github.com/hardbeat920/monocode/commit/be06a81037c99a6c1b6a3eea383eaa21d9b37cc0) — Keep watching for stalled frames while a window needs them
- [84e3d0d5](https://github.com/hardbeat920/monocode/commit/84e3d0d5d0a596c331f20b0e15cd1be0fd46e125) — Glide mouse-wheel notches and stretch chat rows on scroll
- [12552dc8](https://github.com/hardbeat920/monocode/commit/12552dc8b9195e9a15b95ccd805a21ebd5f0d10b) — Add filter field and scrolling to long settings selects
- [4122b598](https://github.com/hardbeat920/monocode/commit/4122b598d69104c7bfef4d4092fc7817f2b7baac) — Raise the Android Mono sheet's header and title a new Mono's Edit page
- [91373752](https://github.com/hardbeat920/monocode/commit/913737524b5926810ab93017b4ccc05c99c46317) — Leave room under the Android sheet's handle for its header
- [42c9763b](https://github.com/hardbeat920/monocode/commit/42c9763b5b00479ba1589adf1eaf9450e7dfa46f) — feat(mobile): add Monos-only layout and recent documents
- [f11fd0a2](https://github.com/hardbeat920/monocode/commit/f11fd0a2bbcc2a7214e7d7eb3fdfb84023e51dfe) — Tint Monos-only chats with the Mono's colour, lift dark theme
- [5501f8b8](https://github.com/hardbeat920/monocode/commit/5501f8b8ae31ce81fcd2d4b1e10dc20069ec0043) — Give Monos an MCP app tool and say what they're doing
- [ed200b85](https://github.com/hardbeat920/monocode/commit/ed200b85f1b99d5ae4e0fedc28f8b74caa680844) — Add a dismissible "Star on GitHub" prompt to the sidebar
- [4c6effe4](https://github.com/hardbeat920/monocode/commit/4c6effe4c4f5259a52451faa8cf67fa0712c947f) — Animate Mono chat transcript rows, reactions and mascot on iOS
- [90dbf061](https://github.com/hardbeat920/monocode/commit/90dbf0619678c3c4752a2daa30d87b092d3677f1) — Move Mono rules into system instructions and default to low effort
- [c12253e2](https://github.com/hardbeat920/monocode/commit/c12253e2cc73234eeac4a4e338aac84ba6a696e7) — Add an option to open global search as a floating command panel
- [309fd9a5](https://github.com/hardbeat920/monocode/commit/309fd9a5a7590c53356395ae2e78891a79b1545f) — Release the keyboard when a Mono's chat closes on iOS
- [dc11aeea](https://github.com/hardbeat920/monocode/commit/dc11aeea218cfb882d898a5ce9b20777f42214f5) — Don't re-take a Mono message its own running turn already holds
- [ea53f94f](https://github.com/hardbeat920/monocode/commit/ea53f94f5235cd890d9d15619f19f41581f82d70) — Resend Mono rules in the message when a resumed Codex thread lacks them
- [6106d410](https://github.com/hardbeat920/monocode/commit/6106d410a31bb111c07dbb6231406600196fc964) — Let a Mono set the model and effort for sessions it starts
- [0b7be7fb](https://github.com/hardbeat920/monocode/commit/0b7be7fbb448ca3c4306b2655cf02307ff0333e7) — Fix mobile and native CI failures
- [f5b2965c](https://github.com/hardbeat920/monocode/commit/f5b2965c1efb04b4c557c34c722a69eaa7a49c71) — Remove elastic wheel-scroll stretch from transcript rows
- [9078735e](https://github.com/hardbeat920/monocode/commit/9078735efc97c266a88c2cf574299374ce3941fc) — Add tile stacks to tab marks and a morphing send button
- [59adb9df](https://github.com/hardbeat920/monocode/commit/59adb9df604a44744f79ca92ba8c1c9ed9fe79a9) — Replace glass tab tiles with solid neutral tiles
- [306cfc35](https://github.com/hardbeat920/monocode/commit/306cfc35d74d454c9eb150be215dc708b4336d69) — Move phase tile into its own view so hovers skip transcript redraw
- [3d522d43](https://github.com/hardbeat920/monocode/commit/3d522d43000f9d56dccff747de1305a0794f1f80) — Add tool group fold settings and tile lone calls and work headers
- [bd96fe80](https://github.com/hardbeat920/monocode/commit/bd96fe800817d43079be2fae42ce5c3dfb6f8921) — Exclude session_index.jsonl.lock from generic Codex config sync
- [5c6cece8](https://github.com/hardbeat920/monocode/commit/5c6cece880db2fce02f584e4c8cfbdac46a7801c) — Refine send button styling and activity rail connections
- [d2d0640c](https://github.com/hardbeat920/monocode/commit/d2d0640ce3f98b6954b738a3cd763ecff941667e) — Use filter_map in the tab tile stack to satisfy clippy
- [b55a623b](https://github.com/hardbeat920/monocode/commit/b55a623bbb67f6c36c520d75e7cac1ec48fc5f87) — Add regression test for session_index.jsonl.lock sync race
- [8d925ee8](https://github.com/hardbeat920/monocode/commit/8d925ee8c18562fec5226f0f3e4a0762322a9d51) — Tighten regression test against a hard-linked lock on Windows
- [b2a5a155](https://github.com/hardbeat920/monocode/commit/b2a5a1551774571eade4d4f54c1ab327d0f3607b) — Drop a stale session_index.jsonl.lock link on sync
- [2476cfcb](https://github.com/hardbeat920/monocode/commit/2476cfcb5b5327c56e577d9662678631b972361f) — Detect and remove a stale hard-linked lock on Windows too
- [e8826f94](https://github.com/hardbeat920/monocode/commit/e8826f943c9618b0c02ebbbede2150a0f6d04fa8) — Make the Windows stale-lock removal TOCTOU-safe
- [4641998f](https://github.com/hardbeat920/monocode/commit/4641998f9b8f9164e02291a4dd5fa233a8a3150e) — Request DELETE access when opening the Windows lock handles
- [7724a2ee](https://github.com/hardbeat920/monocode/commit/7724a2ee21dde574b1988edb38f4fa66ffd1845a) — Redraw the transcript once it learns its width so the prompt outline shows
- [9116f179](https://github.com/hardbeat920/monocode/commit/9116f1796a99027b5142bfa3ef8df104a4f72702) — Limit transcript redraws to prompt outline visibility changes
- [14ecdee7](https://github.com/hardbeat920/monocode/commit/14ecdee7bc5a472f58e16137b7a1a0006fc63814) — Let dock terminals be dragged out into splits or title tabs
- [60409504](https://github.com/hardbeat920/monocode/commit/60409504f854294967fdb0e272e1d54742c99324) — Shuffle stacked tab tiles by pulling two cards apart with a tilt
- [3fd47cf8](https://github.com/hardbeat920/monocode/commit/3fd47cf892f4a3288eb60056be67e056977c3636) — Add pinned title tabs that fan out from a deck at the strip's edge
- [5a31951a](https://github.com/hardbeat920/monocode/commit/5a31951a7439baa564554fe4824570a9522c9202) — Fix native CI checks and preserve video metadata loading
- [8b0de747](https://github.com/hardbeat920/monocode/commit/8b0de747e50898715fbaaebd15ac4010c9dc49d4) — Move the turn's review card to a changes pill above the composer
- [8b5c3326](https://github.com/hardbeat920/monocode/commit/8b5c33263e1865954f164cc1756372d2764f2da9) — Carry pending turn changes into later turns until resolved
- [b1bb0933](https://github.com/hardbeat920/monocode/commit/b1bb09331b5fe5544d048c4cf6e22b02351e973d) — Apply rustfmt to the windows-sys import list
- [d419606e](https://github.com/hardbeat920/monocode/commit/d419606e6e71ece6b1de907372fbafdc148be0c5) — Drop files discarded outside the app from pending turn changes
- [57f8d91c](https://github.com/hardbeat920/monocode/commit/57f8d91c9cc9cc801200e125837806adb1e6ad47) — Move settings search into a collapsible settings nav
- [dabfaa68](https://github.com/hardbeat920/monocode/commit/dabfaa680ac01c8fbd42e4925907569f2f94fbbf) — Add a scripted Claude stream for repeatable CPU profiling
- [08e2b074](https://github.com/hardbeat920/monocode/commit/08e2b0745f674a503ede64caca12d328070943bf) — Reduce top padding in settings navigation
- [a747eb7d](https://github.com/hardbeat920/monocode/commit/a747eb7d8f22cb2839f3a82e40ed0e9303337f72) — Serialize prepare_files and tolerate an already-removed stale lock link
- [7390882a](https://github.com/hardbeat920/monocode/commit/7390882ae588d89061d99ec1a70e2c88d89918c8) — Dim the settings back button and brighten its icon on hover
- [73cd7109](https://github.com/hardbeat920/monocode/commit/73cd710901e2e5697c45aa7c6dde22eab67bf44c) — Format settings search UI and shell tests
- [4550fded](https://github.com/hardbeat920/monocode/commit/4550fdedce9344e41ce5836b2c7e16c1cdcbd471) — Fix changes shelf test initialization for strict Clippy
