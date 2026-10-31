// Injected into every built-in browser page before its own scripts. Keeps the
// last console messages and uncaught errors so agents can read them later.
(function () {
  if (window.__monocodeConsole) return;
  var MAX = 500;
  var entries = [];
  Object.defineProperty(window, "__monocodeConsole", {
    value: entries,
    configurable: false,
    enumerable: false,
  });
  function describe(value) {
    if (typeof value === "string") return value;
    if (value instanceof Error) return value.stack || String(value);
    try {
      var text = JSON.stringify(value);
      return text === undefined ? String(value) : text;
    } catch (_) {
      return String(value);
    }
  }
  function record(level, args) {
    var text = Array.prototype.map.call(args, describe).join(" ");
    if (text.length > 4000) text = text.slice(0, 4000) + "…";
    entries.push({ level: level, text: text, time: Date.now(), url: location.href });
    if (entries.length > MAX) entries.splice(0, entries.length - MAX);
  }
  ["log", "info", "warn", "error", "debug"].forEach(function (level) {
    var original = console[level];
    console[level] = function () {
      record(level, arguments);
      return original.apply(this, arguments);
    };
  });
  window.addEventListener("error", function (event) {
    var where = event.filename ? " (" + event.filename + ":" + event.lineno + ")" : "";
    record("uncaught", [(event.error && event.error.stack) || event.message + where]);
  });
  window.addEventListener("unhandledrejection", function (event) {
    record("unhandledrejection", [event.reason]);
  });
})();
