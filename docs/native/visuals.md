# Native session visuals

An agent can publish an interactive visual into an ordinary conversation. Ask for a chart, comparison, calculator, diagram, or 3D explanation. The visual stays in the transcript with the rest of the reply and remains usable after the turn ends. Monos use the same capability when running a conversation.

Use `/visualize` in an idle local desktop conversation to explicitly request a visual of the current topic, or `/visualize <request>` to specify what to show. Like `/stage` and `/commit`, this local command sends a dedicated agent prompt. It requires reading the visual reference, checking the program and publishing a native inline visual before replying, with the executable fallback available when MCP tools are unavailable. Wait for the current turn and queued messages to finish first. The command is unavailable in remote sessions and Mono chats; ask a Mono for a visual in ordinary language instead. A rejected command preserves the draft and attachments.

Interactive rendering is a native desktop feature. The phone transcript shows the visual's title with an instruction to open it on desktop.

The app renders visual components directly through GPUI. Programs describe content and interactions; the renderer supplies fonts, colors, control appearance, spacing defaults, and responsive sizing. There is no browser surface or HTML document in this path.

## Agent workflow

Active sessions offer `visual_docs`, `visual_check`, and `visual_show` through the session's MCP server where the provider supports injected tools. Tool descriptions are short capability summaries. The full component API, styling rules, examples, and limits are returned by `visual_docs` when the agent prepares a visual. Claude Code receives brief usage guidance in its appended system prompt, and Codex receives it in its developer instructions, when the session's visual bridge is configured. Claude's session-scoped `monocode_visual` tools are allowed without an extra permission prompt, including in read-only and planning turns. No permanent provider configuration is changed.

1. Call `visual_docs` with `{}` to load the reference when creating a visual.
2. Write a JavaScript function body that returns a synchronous render function.
3. Call `visual_check` with `{ "source": "..." }` and fix any reported execution or component errors.
4. Call `visual_show` with `{ "title": "Short descriptive title", "source": "..." }` during the current turn. An optional `id` identifies a publication.
5. Add supporting prose without repeating what the visual already explains.

Pi, omp, and OpenCode use the executable fallback. Grok and fx also receive fallback discovery when resuming through a protocol that cannot refresh MCP tools. These paths get a compact, optional CLI reference once per provider conversation, including existing conversations on their first successful turn after the update. A saved receipt prevents repeating it; a new provider conversation, account, or executable receives a new reference. The SDK is loaded only through `visual docs` or `visual --help`. Ordinary replies use visuals when useful; `/visualize` explicitly requests a publication during that turn.

The check executes the program and validates its native component tree and representative controls. It does not produce a screenshot. Publishing validates again before saving the visual and acknowledging the request.

The equivalent commands work with a source file:

```sh
monocode-gpui visual docs
monocode-gpui visual check --input /path/to/explanation.js
monocode-gpui visual show --input /path/to/explanation.js --title "Explanation"
```

Reading the reference and checking also work outside a running app. Showing requires the capability inherited by the agent of a live session. CLI discovery supplies the executable path; it is also available to the agent through `MONOCODE_VISUAL_EXECUTABLE`.

## Mono documents

A visual published in a Mono chat also appears in that Mono's **Documents** tab.
Opening it uses the same native interactive reader. Reusing the visual's `id`
updates the existing document while preserving its original creation time.

The Mono app API accepts `artifacts.write` with `kind: "document"` for Markdown
documents. Creating an artifact with `kind: "html"` is rejected; publish
interactive content through `visual_show` or the equivalent executable command.

## Programs and native controls

```js
const state = visual.state({ quantity: 4, price: 12 });

return () => visual.column([
  visual.text("Cost explorer", { size: "title" }),
  visual.stat("Total", "$" + state.quantity * state.price),
  visual.slider("Quantity", state.quantity, 1, 20,
    value => { state.quantity = value; }),
  visual.button("Reset", () => { state.quantity = 4; }),
], { gap: 16 });
```

Handlers update local state and the app rerenders the component tree. The script interpreter persists while that view is retained. Reset recreates its initial state. `Math.random()` starts from a repeatable source-based seed, so generated example data is consistent when the program is checked or reopened. The header icons switch to the syntax-highlighted JavaScript source and reset the visual; source mode also offers a copy icon. Visuals grow to their full content height and scroll with the conversation. The source viewer scrolls long programs within its code pane.

| Component | Signature |
| --- | --- |
| Layout | `column(children, {gap})`, `row(children, {gap})`, `grid(children, {columns, gap})` |
| Text | `text(value, {size, color})`, `stat(label, value, {detail, color})` |
| Data | `table(columns, rows)`, `chart(type, series, {labels, height})` |
| Actions | `button(label, handler, {disabled})`, `toggle(label, value, handler)` |
| Values | `slider(label, value, min, max, handler, {step})`, `input(label, value, handler, {placeholder})` |
| Choices | `tabs(labels, selectedIndex, handler)`, `select(label, options, selectedIndex, handler)` |
| Drawing | `canvas(shapes, {width, height})` |
| Geometry | `scene(objects, {width, height, camera})` |

All functions above belong to `visual`. Option objects and their fields are optional unless the component needs the value. Chart types are `bar`, `line`, and `area`. Each series has a `label`, numeric `values`, and an optional semantic `color`. All series have equal lengths; labels, when supplied, match that length.

Canvas drawing uses `rect`, `circle`, `line`, `polyline`, and `label`. Coordinates refer to the declared canvas width and height, and the host scales the drawing to fit the conversation.

## Styling

Use `ink`, `muted`, `faint`, `accent`, `positive`, `negative`, `warning`, and `series1` through `series6` for colors. Use `caption`, `small`, `body`, `subtitle`, `title`, or `hero` for text sizes. Unsupported style properties and arbitrary color strings fail validation.

Let the app draw buttons, inputs, switches, and choice controls. Use layout components rather than painting controls onto a canvas. Prefer gaps around 8–16, restrained headings, clear units, and chart or scene heights around 220–360. The outer visual inherits the conversation's appearance rather than introducing a page background.

## 3D geometry

Scenes accept boxes, spheres, cylinders, cones, planes, custom triangle meshes, and 3D polylines. Each object has a position, rotation in radians, scale, and semantic color. Mesh indices are triples of vertex indices. Geometry is transformed, clipped at the camera's near plane, lit, projected, and drawn with native GPUI paths. Dragging orbits the scene; Command/Control plus scroll changes its camera distance. Ordinary scrolling stays with the transcript.

```js
return () => visual.scene([
  visual.object({ kind: "box" }, {
    rotation: [0.2, 0.5, 0], scale: [2, 1, 1], color: "accent",
  }),
  visual.object({
    kind: "mesh",
    vertices: [[-1, 0, 0], [1, 0, 0], [0, 2, 0]],
    indices: [[0, 1, 2]],
  }, { position: [2, 0, 0], color: "series2" }),
], { height: 320 });
```

This renderer supports bounded geometry rather than arbitrary GPU shaders, external model files, or texture assets. Controls can alter geometry and camera parameters through state.

## Lifetime and limits

Visual source is saved as a dedicated transcript message, including programs split across storage chunks. Its native row remains outside folded tool activity. A loaded transcript keeps at most eight visual interpreters, discarding the least recently painted views as needed. Reopening a discarded view or restarting the app recreates the program's initial state; control state is not written into the conversation.

Each interpreter has a 16 MiB memory budget and a 60 ms execution deadline per call. Source is limited to 64 KiB, trees to 512 components and 16 nesting levels. Tables, chart series, drawing commands, text, and mesh geometry have additional validated limits. Invalid or interrupted programs show an error in their own view and can be reset.

Programs have no filesystem, network, process, DOM, module loading, or timer bindings. Controls change only their visual's local state. Publication is scoped to the calling session and requires an active turn; it cannot target another conversation.

## Local examples

The [dashboard example](examples/visual-dashboard.js) exercises charts, tabs, stats, sliders, switches, and tables. The [geometry example](examples/visual-scene.js) exercises 3D primitives, orbiting, geometry controls, and 2D drawing. Both examples are checked against the real interpreter in tests.

To seed them into an isolated development conversation:

```sh
cargo run --manifest-path crates/monocode-gpui/Cargo.toml --locked -p monocode-gpui --profile native-dev -- --dev-data --visual-fixture
```

The fixture cannot be written into the production database.
