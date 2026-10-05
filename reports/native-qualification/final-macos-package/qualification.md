# Final frozen Mac package

The [runner summary](summary.json) records a complete arm64 Mac package check with every command exiting zero. It copied the exact app, host, and package tool hashes from the [completed source qualification](qualification-summary.json) into an owned artifact directory before packaging. The [source-before check](logs/source-before.log) and [source-after check](logs/source-after.log) both report 2,629 files with zero differences. Their source digest is `58c060e4687dc6bf600d89ce6423f2682e25355ea2867406ef3d996bb544a4c0`.

The artifacts remain on hometop under `/Users/gianvillarini/Library/Caches/monocode-gpui-build/artifacts/final-macos-package-l41jg_7z`. This report retains all command logs, hashes, the source manifest, and the rendered image. It does not copy the large packages into the local checkout.

The [package verification](package-verification.json) passed ad hoc signature inspection, strict bundle signature verification, plist identity and version checks, exact arm64 architecture, and UUID comparison with the qualified inputs. The host archive contains the unchanged input host. The app archive matches the signed bundle's executables, metadata, and icon assets. A read-only DMG mount contains the same bundle and an `/Applications` symlink. Every listed artifact checksum passed.

| Artifact | SHA-256 |
| --- | --- |
| `MonoCode_0.6.0_aarch64-apple-darwin.app.tar.gz` | `76339b33d7062437da4792198140caeb2d05530f36a9e0be7a89a640cf0e78eb` |
| `MonoCode_0.6.0_aarch64-apple-darwin.dmg` | `fa8b7b20b03541f99c91f596ea125c05f5011c5f46d659d6d589085846e38cb5` |
| `monocode-host_0.6.0_aarch64-apple-darwin.tar.gz` | `0cfa5937cb08554f48fa296e88f215c9ac0377a84078b8927b9ed641d82ead49` |

The [private updater fixture](logs/private-updater.log) used the actual packaged app archive, a disposable signing key, and a loopback feed. A changed archive failed signature verification and left the temporary original bundle intact. The trusted archive replaced that same temporary bundle. Its executable hash and plist matched the archive, bundled host version and app CLI checks passed, and the installed bundle passed strict signature verification. The fixture removed its keys and temporary installation. It did not change the source package or any production feed or installation.

The [packaged renderer log](logs/packaged-renderer.log) records a successful 2560 by 1600 PNG. The [image](packaged-widgets.png) has SHA-256 `78d583ed5687bf7db9c5c21e2409900b0242754c9069e9a9070729b47a9cceb0`. The app integration agent and root coordinator independently inspected it. Their [review](visual-review.json) passed the expected buttons, SVG icon buttons, badges, diff statistics, switches, segmented controls, text fields, menu, popover, tooltip, and static approval toast. The `widgets` view does not boot the engine. The runner supplied a private scratch profile and disabled scheduled agents, so it imported no user settings and started no provider or automation. This static fixture did not exercise an actual Claude approval.

These artifacts use the qualified development binaries with debug information disabled. They have an ad hoc signature and no notarization. The renderer check supplies visual evidence for the packaged widget scene. It does not qualify literal desktop input, permissions, the Quick composer, a signed release identity, or a production updater feed.
