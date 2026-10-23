# Example mods

Each folder is a MonoMod: a `mod.json` and a `register(on, $)` entry. See [docs/monomods.md](../../docs/monomods.md) for the design and [types.ts](../../src/features/mods/api/types.ts) for the API.

- `hello/`: the smallest mod. A sidebar tab, a status chip, a toast after each turn, and a band above the prompt.

## Load one

1. Run Monocode (`npm run tauri dev`).
2. Open Settings > Mods, click **Load unpacked** and pick the mod folder, e.g. `examples/mods/hello`.
3. Toggle it on and accept the permissions sheet.

After editing the source, click **Reload** on the mod. To install a mod for good, copy its folder to `~/.monocode/mods/`, or to `<project>/.monocode/mods/` for one project.
