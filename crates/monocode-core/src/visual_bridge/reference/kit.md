Optional kit. Every HTML page includes MonoCode's component kit. Use any
helpers that fit the subject, or write custom HTML, CSS, SVG and JavaScript.
UI mockups can use the product's own visual identity.
The parts worth reaching for are the ones a page cannot easily make itself:
mc.clock (slow motion and pause for everything on the page), mc.state (values
the agent reads and sets), mc.send (a line to the agent from the person), the
theme variables and the classes that look like the app's own controls.
Ready-made bars (mc.variants, mc.transport, mc.controls) are examples of one
shape; when the subject wants different controls, build those instead, from the
same parts.
Classes: mc-row, mc-col, mc-grid, mc-spacer, mc-muted, mc-label, mc-mono,
mc-title, mc-divider; mc-card (tinted panel), mc-surface (floating popover with
shadow); mc-button (+ primary, accent, ghost, danger, icon, small), mc-input,
mc-badge (+ accent, success, warning, danger), mc-kbd, mc-avatar; mc-menu with
mc-menu-item (+ active, danger; an mc-kbd inside is right-aligned) and
mc-menu-sep; mc-stat (label text then <b>value</b>); mc-window with mc-window-bar
(three <i></i> for traffic lights) for app mockups; mc-stage (+ dots or
grid-lines) as the backdrop a subject sits on; mc-tabs with mc-tab
(aria-selected); mc-switch (role=switch, aria-checked); mc-skeleton; motion
classes mc-in and mc-pop that stagger by the --i custom property.
Script helpers on window.mc:
- mc.h(tag, attrs, ...children) builds elements; attrs take class, style
  objects, onclick-style handlers and plain attributes.
- mc.icon(name, {size?}) returns an inline SVG icon; mc.icons lists the names
  (check x plus minus chevron-* arrow-right arrow-left search settings user file
  folder trash copy edit play pause replay more menu bell mail home star heart
  link lock calendar clock download upload filter sun moon info alert sparkle
  terminal code git).
- mc.variants(target, [{name, note?, render(stage, ctx)}], {title?, stage?,
  stageHeight?, controls?, replay?, playback?, autoplay?}) compares alternatives,
  mostly motion or interaction designs: a plain stage (pass {stage:'dots'} or
  'grid-lines' for a pattern), tabs and a Replay button when there are several
  variants (replay:true forces Replay, replay:false removes it), a one-line note
  each, and with {playback:true} a speed slider that slows every CSS and Web
  Animations animation. render fills the stage and may return a cleanup function; it
  runs again on every switch and replay, so entrance animations replay for free.
  ctx.now() is playback-scaled time for requestAnimationFrame loops. Arrow keys
  switch variants, r replays. A single variant has no chrome at all.
  {layout:'side'} shows every variant at once in one row, each with its name and
  note above its own stage, so one trigger drives them together: the right shape
  for versions of one component, where tabs would hide the difference. render
  gets ctx.states (the o.states you passed) and ctx.clock; return the function
  from states.on(...) as the cleanup. Use tabs for different pages.
- mc.clock is the page's one clock: speed, paused, setSpeed(v), pause(), resume(),
  on(f). Slow motion also scales performance.now, Date.now, requestAnimationFrame
  timestamps and setTimeout/setInterval delays, so script-driven motion (GSAP,
  hand-written loops) slows with the CSS. Paused freezes animations and those clocks.
- mc.states(['expanded','collapsed'], {initial?}) is a named state shared by the
  page: name, go(nameOrIndex), next(), on(f) (runs now and on every change,
  returns a stop function). Each variant listens and animates its own way.
- mc.transport(target, {states?, labels?, replay?, slow?, speeds?, autoplay?,
  every?, pause?}) is the playback bar, for judging motion only: a button that
  flips states and is labelled with the next one (labels: {collapsed:'Collapse',
  expanded:'Expand'}), Replay, a "Slow motion ×4" switch (slow: N) or speed chips
  (speeds: [1,.5,.25]), Auto-play (loops the trigger every `every` ms, default
  1600) and Pause. Returns an element with destroy().
- mc.slider(label, {min,max,step,value,unit?,format?,onInput}),
  mc.toggle(label, value, onChange), mc.tabs(labels, selected, onChange) return
  ready controls; put them in the variants controls option or your own bar.
- mc.state({key: value}) is an optional store for what the person changes with
  controls: get, set, patch, reset, on(f) (runs now and on every change, returns
  a stop function) and values. Each key is mirrored to a CSS custom property
  (radius -> --s-radius, numbers unitless, so calc(var(--s-radius) * 1px)) and
  each boolean to a data attribute on <html> (glow -> data-s-glow="on"), so
  plain CSS can react with no script. The values reach you as `state` in the
  next draft snapshot: look at it to see what the person tried. You change
  them with draft_preview {draft, action:{kind:"set", values:{...}}}; keys the
  store does not have are refused. A page has one store: mc.state() returns
  it, and a later mc.state({...}) or mc.controls adds its new keys to the same
  store.
- mc.send(text) sends a line to the agent as from the person, for a control
  such as "Use this one". It works only inside the handler of the person's own
  click or key press (within two seconds) and returns whether it went.
- mc.controls(target, spec, {state?, layout?, title?}) puts controls outside
  the content, bound to an mc.state (one is made from the specs' `value`s when
  you pass none; read it as el.state). Specs: {key, type, label?, onChange?}
  with type slider {min,max,step,unit,format}, text or textarea {placeholder},
  toggle, checkbox, segmented, radio or select {options: ['a','b'] or
  [{value,label}]}, stepper {min,max,step,prefix,unit}, swatch {options:
  [colors]}; plus {type:'button', label, icon?, primary?, onClick} and
  {type:'sep'}. layout is 'rail' (a column beside the content, with title), 'bar'
  (a toolbar above) or 'dock' (floats at the bottom of a positioned parent).
  Returns an element with destroy(). Never required: mc.slider, mc.toggle and
  plain inputs still work on their own.
- mc.stagger(elements) sets --i on each; mc.series(i) is chart color i.
- mc.chart(target, {type, labels?, series?, values?, height?, x?, y?, legend?,
  curve?, animate?}) draws a themed, responsive SVG chart with hover tooltips, a
  crosshair and a legend that toggles series. type: line, area, bar (grouped),
  stacked, hbar (horizontal; labels down the side), scatter or donut.
  series: [{name, values, color?}], values aligned with labels; scatter values
  are [x, y, r?, label?]; donut takes labels and values (plus format, center,
  centerLabel). x and y take {label, format, min, max, zero}; format is a
  function or a unit like '$', '%' or ' ms'. Returns {el, update(partialSpec)}
  for controls that change the data. Prefer it to a CDN chart library.
- mc.flow(target, {nodes:[{id, label, detail?, icon?, tone?}], edges:[[from, to,
  label?] or {from, to, label, dashed}], direction?, onSelect?}) lays out a
  diagram from its edges: ranks run left to right and wrap to new rows, or
  direction 'down' stacks them. tone: accent, success, warning, danger, muted.
  Returns {el, focus(ids)}; focus highlights those nodes and animates the edges
  between them.
- mc.steps(target, [{title, body, focus?, render?(el)}], {flow?, autoplay?,
  initial?}) is a stepper with previous/next, progress and arrow keys; with
  {flow} each step focuses its nodes, and render can draw anything per step.
- mc.code(source, {lang, title?, numbers?, highlight?: [lines], wrap?}) returns a
  highlighted code block (js/ts, rust, python, go, c-like, sh, sql, css, json,
  html/xml, diff); mc.highlight(source, lang) returns the HTML.
- mc.table(rows, {columns?: [key or {key, label, format(v,row)}], sort?,
  maxHeight?}) returns a sortable table; numbers align right.
- mc.compare(target, {before, after, labels?, initial?, onInput?}) stacks two
  renderings (elements, HTML strings or functions) under a draggable handle that
  reveals the second; returns {el, set(percent)}.
- mc.annotate(target, {content or src, alt?, notes:[{x, y, title, text?}]}) puts
  numbered pins at x/y percentages of any picture or element, with a note list that
  lights up with them; returns {el, pick(i)}.
- mc.timeline(target, {events:[{at, end?, label, detail?, tone?}], format?, min?,
  max?}) lays events on a numeric axis (minutes, milliseconds, years), spans when
  `end` is set, overlaps stacked into lanes. tone: accent, success, warning, danger,
  muted, info.
- mc.spark(values, {width?, height?, color?, min?, max?, fill?, last?, label?})
  returns an inline SVG trend for stats, tables and prose. Give a shared min and
  max when rows must compare honestly.
- mc.scrub(target, {min, max, value?, step?, label?, format?, onInput, duration?,
  loop?, autoplay?}) is a time scrubber for when time is the point; duration (ms)
  adds a Play button. Returns {el, set, play, pause}.
- mc.data is the data files passed with the visual (see Data files).
Images in a page open in the app's image viewer when clicked, unless they sit in
a link or button or the page handles the click itself.

Data files. To chart or tabulate real data, pass {data: {name: "/abs/path"}} to
visual_check, visual_preview and visual_show (CLI input JSON takes the same
field). Up to 8 files of 2 MiB each: .json is parsed, .csv and .tsv become
arrays of row objects keyed by the header (numbers as numbers, empty cells as
null, leading-zero codes kept as text), anything else arrives as a string. The
page reads them as mc.data.name. Aggregate large data first (with a script) and
pass the result; never paste big datasets into the HTML.

Size by content: an mc.variants stage is at least 320 points; pass
{stageHeight} to change it. The stage keeps the tallest height its content
reaches, so a replay or tab switch that starts smaller doesn't move what's below
it. Hand-built demos that reset should do the same: give their stage a
min-height at least as tall as their largest state. Respect
prefers-reduced-motion (the kit already shortens CSS motion). A script error
leaves a blank page, so preview every time.
