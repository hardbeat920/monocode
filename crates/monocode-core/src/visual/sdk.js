(() => {
  "use strict";
  // A native Atomics.wait bypasses JavaScript interrupts. Shared memory and
  // blocking primitives have no purpose in a synchronous visual program.
  for (const name of ["Atomics", "SharedArrayBuffer"]) {
    Object.defineProperty(globalThis, name, {value: undefined, writable: false, configurable: false});
  }
  // Keep the bridge private: a program can only return component data and
  // register callbacks; it cannot call or replace the host's dispatch entrypoint.
  const stringify = JSON.stringify.bind(JSON);
  const parse = JSON.parse.bind(JSON);
  const FunctionBody = Function;
  const handlers = new Map();
  let generation = 0;
  let renderView;
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  const options = (value, allowed) => {
    if (value === undefined) return {};
    assert(value && typeof value === "object" && !Array.isArray(value), "Options must be an object");
    for (const key of Object.keys(value)) assert(allowed.includes(key), `Unsupported style or option: ${key}`);
    return value;
  };
  const text = (value) => {
    assert(typeof value === "string" || typeof value === "number", "Text must be a string or number");
    return String(value);
  };
  const register = (fn, accepts, disabled = false) => {
    assert(typeof fn === "function", "Interactive controls require a function handler");
    assert(handlers.size < 512, "Too many visual handlers");
    const id = `${generation}:${handlers.size}`;
    handlers.set(id, { fn, accepts, disabled });
    return id;
  };
  const children = value => {
    assert(Array.isArray(value), "Layout children must be an array");
    assert(value.length <= 512, "Too many layout children");
    return value;
  };
  const layout = (kind, nodes, o) => {
    o = options(o, kind === "grid" ? ["gap", "columns"] : ["gap"]);
    return {kind, children: children(nodes), gap: o.gap ?? 12,
      ...(kind === "grid" ? {columns: o.columns ?? 2} : {})};
  };
  const color = o => o.color ?? "accent";
  const api = {
    state(initial) {
      assert(initial && typeof initial === "object" && !Array.isArray(initial), "State must be an object");
      return initial;
    },
    column: (nodes, o) => layout("column", nodes, o),
    row: (nodes, o) => layout("row", nodes, o),
    grid: (nodes, o) => layout("grid", nodes, o),
    card: (nodes, o) => layout("card", nodes, o),
    text(value, o) {
      o = options(o, ["size", "color"]);
      return {kind: "text", text: text(value), size: o.size ?? "body", color: o.color ?? "ink"};
    },
    stat(label, value, o) {
      o = options(o, ["detail", "color", "delta", "trend", "better"]);
      assert(o.better === undefined || o.better === "higher" || o.better === "lower", "Stat better must be 'higher' or 'lower'");
      assert(o.trend === undefined || Array.isArray(o.trend), "Stat trend must be an array of numbers");
      return {kind: "stat", label: text(label), value: text(value), detail: o.detail == null ? null : text(o.detail),
        color: o.color ?? "ink", delta: o.delta == null ? null : text(o.delta), trend: o.trend ?? [],
        lower_is_better: o.better === "lower"};
    },
    gauge(label, value, o) {
      o = options(o, ["min", "max", "unit", "color"]);
      return {kind: "gauge", label: text(label), value, min: o.min ?? 0, max: o.max ?? 100,
        unit: o.unit == null ? null : text(o.unit), color: color(o)};
    },
    donut(slices, o) {
      o = options(o, ["label", "value"]);
      assert(Array.isArray(slices), "Donut slices must be an array");
      return {kind: "donut", label: o.label == null ? null : text(o.label), value: o.value == null ? null : text(o.value),
        slices: slices.map((s, i) => {
          options(s, ["label", "value", "color"]);
          return {label: text(s.label), value: s.value, color: s.color ?? `series${i % 6 + 1}`};
        })};
    },
    heatmap(values, o) {
      o = options(o, ["rows", "columns", "color"]);
      assert(Array.isArray(values) && values.every(Array.isArray), "Heatmap values must be an array of rows");
      return {kind: "heatmap", values, rows: (o.rows ?? []).map(text), columns: (o.columns ?? []).map(text),
        color: o.color ?? "positive"};
    },
    progress(label, value, o) {
      o = options(o, ["max", "detail", "color"]);
      return {kind: "progress", label: text(label), value, max: o.max ?? 100,
        detail: o.detail == null ? null : text(o.detail), color: color(o)};
    },
    table(columns, rows) {
      assert(Array.isArray(columns) && Array.isArray(rows), "Table columns and rows must be arrays");
      return {kind: "table", columns: columns.map(text), rows: rows.map(row => {
        assert(Array.isArray(row), "Table rows must be arrays"); return row.map(text);
      })};
    },
    chart(chart_type, series, o) {
      o = options(o, ["labels", "height"]);
      assert(Array.isArray(series), "Chart series must be an array");
      return {kind: "chart", chart_type, labels: (o.labels ?? []).map(text), height: o.height ?? 240,
        series: series.map((s, i) => {
          options(s, ["label", "values", "color"]);
          return {label: text(s.label), values: s.values, color: s.color ?? `series${i % 6 + 1}`};
        })};
    },
    button(label, fn, o) {
      o = options(o, ["disabled"]);
      const disabled = o.disabled ?? false;
      return {kind: "button", label: text(label), disabled,
        handler: register(fn, v => v === null, disabled)};
    },
    toggle(label, value, fn) {
      return {kind: "toggle", label: text(label), value,
        handler: register(fn, v => typeof v === "boolean")};
    },
    input(label, value, fn, o) {
      o = options(o, ["placeholder"]);
      return {kind: "input", label: text(label), value: text(value),
        placeholder: o.placeholder == null ? null : text(o.placeholder),
        handler: register(fn, v => typeof v === "string" && v.length <= 4096)};
    },
    select(label, labels, selected, fn) {
      assert(Array.isArray(labels), "Select options must be an array");
      return {kind: "select", label: text(label), options: labels.map(text), selected,
        handler: register(fn, v => Number.isInteger(v) && v >= 0 && v < labels.length)};
    },
    slider(label, value, min, max, fn, o) {
      o = options(o, ["step"]);
      return {kind: "slider", label: text(label), value, min, max, step: o.step ?? 1,
        handler: register(fn, v => typeof v === "number" && Number.isFinite(v) && v >= min && v <= max)};
    },
    tabs(labels, selected, fn) {
      assert(Array.isArray(labels), "Tab labels must be an array");
      return {kind: "tabs", labels: labels.map(text), selected,
        handler: register(fn, v => Number.isInteger(v) && v >= 0 && v < labels.length)};
    },
    canvas(shapes, o) {
      o = options(o, ["width", "height"]);
      assert(Array.isArray(shapes), "Canvas shapes must be an array");
      return {kind: "canvas", shapes, width: o.width ?? 640, height: o.height ?? 320};
    },
    rect(x, y, width, height, o) {
      o = options(o, ["color"]); return {kind: "rect", x, y, width, height, color: color(o)};
    },
    circle(x, y, radius, o) {
      o = options(o, ["color"]); return {kind: "circle", x, y, radius, color: color(o)};
    },
    line(x1, y1, x2, y2, o) {
      o = options(o, ["width", "color"]); return {kind: "line", x1, y1, x2, y2, width: o.width ?? 2, color: color(o)};
    },
    polyline(points, o) {
      o = options(o, ["width", "color"]); return {kind: "polyline", points, width: o.width ?? 2, color: color(o)};
    },
    label(x, y, value, o) {
      o = options(o, ["color"]); return {kind: "label", x, y, text: text(value), color: o.color ?? "ink"};
    },
    object(geometry, o) {
      o = options(o, ["position", "rotation", "scale", "color"]);
      return {geometry, position: o.position ?? [0, 0, 0], rotation: o.rotation ?? [0, 0, 0],
        scale: o.scale ?? [1, 1, 1], color: color(o)};
    },
    scene(objects, o) {
      o = options(o, ["width", "height", "camera"]);
      assert(Array.isArray(objects), "Scene objects must be an array");
      const c = options(o.camera, ["eye", "target", "fov"]);
      return {kind: "scene", objects, width: o.width ?? 640, height: o.height ?? 360,
        camera: {eye: c.eye ?? [5, 4, 6], target: c.target ?? [0, 0, 0], fov: c.fov ?? 45}};
    },
  };
  Object.freeze(api);
  Object.defineProperty(globalThis, "visual", {value: api, writable: false, configurable: false});
  const render = () => {
    handlers.clear();
    generation++;
    const node = renderView();
    assert(node && typeof node === "object" && typeof node.then !== "function", "Render must synchronously return a component");
    const result = stringify(node, (_key, value) => {
      assert(typeof value !== "number" || Number.isFinite(value), "Visual numeric values must be finite");
      assert(typeof value !== "function" && typeof value !== "symbol" && typeof value !== "bigint", "Visual nodes must contain only component data");
      return value;
    });
    assert(typeof result === "string" && result.length <= 1024 * 1024, "Visual tree exceeds 1 MiB");
    return result;
  };
  return Object.freeze({
    init(source) {
      // Source-seeded randomness makes setup data reproducible across checking,
      // publication and reload. The stream advances normally within this VM.
      const multiply = Math.imul;
      let seed = 2166136261;
      for (let i = 0; i < source.length; i++) seed = multiply(seed ^ source.charCodeAt(i), 16777619) >>> 0;
      Object.defineProperty(Math, "random", {writable: false, configurable: false, value: () => {
        seed = (multiply(seed, 1103515245) + 12345) >>> 0;
        return (seed >>> 1) / 2147483648;
      }});
      renderView = FunctionBody("visual", `"use strict";\n${source}`)(api);
      assert(typeof renderView === "function", "Visual program must return a render function: return () => visual.text('Hello')");
    },
    render,
    dispatch(id, payload) {
      const entry = handlers.get(id);
      assert(entry, "This visual control is stale; use the current view");
      assert(!entry.disabled, "This visual control is disabled");
      const value = parse(payload);
      assert(entry.accepts(value), "Invalid value for this visual control");
      const result = entry.fn(value);
      assert(!result || typeof result.then !== "function", "Visual handlers must be synchronous");
      return render();
    },
  });
})()
