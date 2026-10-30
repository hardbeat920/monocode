// Runs inside the phone's terminal WebView. The app writes shell output with
// window.__mono.write and hears about typing and size through postMessage.
(function () {
  var post = function (message) {
    window.ReactNativeWebView.postMessage(JSON.stringify(message));
  };
  var toBase64 = function (bytes) {
    var text = "";
    for (var i = 0; i < bytes.length; i += 0x8000)
      text += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
    return btoa(text);
  };
  var fromBase64 = function (data) {
    var text = atob(data);
    var bytes = new Uint8Array(text.length);
    for (var i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i);
    return bytes;
  };

  // The desktop terminal's palettes, so the phone reads the same.
  var ANSI_DARK = {
    black: "#1d2428",
    red: "#f87171",
    green: "#4ade80",
    yellow: "#fbbf24",
    blue: "#60a5fa",
    magenta: "#c084fc",
    cyan: "#22d3ee",
    white: "#e8eef2",
    brightBlack: "#64748b",
    brightRed: "#fca5a5",
    brightGreen: "#86efac",
    brightYellow: "#fde68a",
    brightBlue: "#93c5fd",
    brightMagenta: "#d8b4fe",
    brightCyan: "#67e8f9",
    brightWhite: "#f8fafc",
  };
  // One Light, tuned for a near-white canvas.
  var ANSI_LIGHT = {
    black: "#383a42",
    red: "#e45649",
    green: "#50a14f",
    yellow: "#c18401",
    blue: "#4078f2",
    magenta: "#a626a4",
    cyan: "#0184bc",
    white: "#fafafa",
    brightBlack: "#7c8591",
    brightRed: "#df6b60",
    brightGreen: "#68b567",
    brightYellow: "#d19a2f",
    brightBlue: "#5c89f5",
    brightMagenta: "#b54bb3",
    brightCyan: "#1f9cc9",
    brightWhite: "#ffffff",
  };
  // The app's scheme: set before the page loads, and again when it changes.
  var themeFor = function (look) {
    var theme = look.light ? ANSI_LIGHT : ANSI_DARK;
    var out = {};
    for (var name in theme) out[name] = theme[name];
    out.background = look.background;
    out.foreground = look.foreground;
    out.cursor = look.cursor;
    out.cursorAccent = look.background;
    out.selectionBackground = look.light ? "rgba(0,0,0,0.15)" : "rgba(255,255,255,0.18)";
    return out;
  };
  var paint = function (look) {
    document.documentElement.style.background = look.background;
    document.body.style.background = look.background;
  };
  var look = window.__monoLook || {
    light: false,
    background: "#0f0f0f",
    foreground: "#ebebeb",
    cursor: "#4c9ef5",
  };
  paint(look);

  var term = new Terminal({
    cursorBlink: true,
    cursorStyle: "bar",
    fontFamily: "Menlo, ui-monospace, SFMono-Regular, Monaco, monospace",
    fontSize: 15,
    lineHeight: 1,
    scrollback: 5000,
    smoothScrollDuration: 0,
    theme: themeFor(look),
  });
  var fit = new FitAddon.FitAddon();
  term.loadAddon(fit);
  term.open(document.getElementById("terminal"));

  // The hidden textarea takes the keyboard's text: keep the phone from
  // correcting or capitalising what is typed into a shell.
  var area = term.textarea;
  if (area) {
    area.setAttribute("autocapitalize", "none");
    area.setAttribute("autocorrect", "off");
    area.setAttribute("autocomplete", "off");
    area.setAttribute("spellcheck", "false");
  }

  var encoder = new TextEncoder();
  term.onData(function (text) {
    post({ t: "input", data: toBase64(encoder.encode(text)) });
  });
  // Mouse reports and other binary replies are bytes in a string.
  term.onBinary(function (text) {
    var bytes = new Uint8Array(text.length);
    for (var i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i) & 255;
    post({ t: "input", data: toBase64(bytes) });
  });

  var sent = "";
  var report = function () {
    var size = term.cols + "x" + term.rows;
    if (size === sent) return;
    sent = size;
    post({ t: "size", cols: term.cols, rows: term.rows });
  };
  // History scrolls in a native scroll area over the terminal, as tall as the
  // history, rather than in xterm's own touch scrolling, which is script: so
  // it has iOS's momentum and bounce, and iOS sees a drag on it as a scroll
  // rather than one for the sheet. A full-screen program (an editor, a pager)
  // has no history, and its touches go to xterm.
  // The app's key bar is glass over the bottom of the page. The screen ends
  // above it, and under it go the lines after the screen's last, once the view
  // is scrolled back, so history runs on under the bar as a list does. At the
  // live end there is nothing after, and it is empty.
  var inset = 0;
  var overhang = document.getElementById("overhang");
  var NAMES = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    "brightBlack", "brightRed", "brightGreen", "brightYellow",
    "brightBlue", "brightMagenta", "brightCyan", "brightWhite",
  ];
  var CUBE = [0, 95, 135, 175, 215, 255];
  var colorOf = function (cell) {
    if (cell.isFgDefault()) return "";
    var n = cell.getFgColor();
    if (cell.isFgRGB()) return "#" + ("00000" + n.toString(16)).slice(-6);
    if (n < 16) return term.options.theme[NAMES[n]];
    if (n < 232) {
      n -= 16;
      return "rgb(" + CUBE[Math.floor(n / 36)] + "," + CUBE[Math.floor(n / 6) % 6] + "," + CUBE[n % 6] + ")";
    }
    var grey = 8 + (n - 232) * 10;
    return "rgb(" + grey + "," + grey + "," + grey + ")";
  };
  var cell = null;
  var drawOverhang = function () {
    var buffer = term.buffer.active;
    var from = buffer.viewportY + term.rows;
    var to = Math.min(buffer.length, from + Math.ceil(inset / (row || 1)));
    if (!row || from >= to) {
      if (overhang.firstChild) overhang.textContent = "";
      return;
    }
    cell = cell || buffer.getNullCell();
    var lines = document.createDocumentFragment();
    for (var y = from; y < to; y++) {
      var line = buffer.getLine(y);
      var div = document.createElement("div");
      var text = "";
      var color = "";
      var end = function () {
        if (!text) return;
        var span = document.createElement("span");
        span.textContent = text;
        if (color) span.style.color = color;
        div.appendChild(span);
        text = "";
      };
      for (var x = 0; line && x < line.length; x++) {
        line.getCell(x, cell);
        if (cell.getWidth() === 0) continue;
        var next = colorOf(cell);
        if (next !== color) {
          end();
          color = next;
        }
        text += cell.getChars() || " ";
      }
      end();
      div.style.height = row + "px";
      lines.appendChild(div);
    }
    overhang.textContent = "";
    overhang.appendChild(lines);
  };
  var placeOverhang = function () {
    var screen = document.querySelector(".xterm-screen");
    if (!screen) return;
    var box = screen.getBoundingClientRect();
    overhang.style.top = box.bottom + "px";
    overhang.style.left = box.left + "px";
    overhang.style.width = box.width + "px";
    overhang.style.bottom = "0";
    overhang.style.font = term.options.fontSize + "px " + term.options.fontFamily;
    overhang.style.lineHeight = row + "px";
    overhang.style.color = term.options.theme.foreground;
  };

  var scroller = document.getElementById("scroller");
  var tall = document.getElementById("tall");
  var row = 0;
  var lineAt = function () {
    var top = Math.round(scroller.scrollTop / row);
    return Math.max(0, Math.min(term.buffer.active.baseY, top));
  };
  var sync = function () {
    if (!row) return;
    var buffer = term.buffer.active;
    scroller.style.pointerEvents = buffer.type === "alternate" ? "none" : "auto";
    tall.style.height = Math.round(scroller.clientHeight + buffer.baseY * row) + "px";
    if (lineAt() !== buffer.viewportY) scroller.scrollTop = buffer.viewportY * row;
    drawOverhang();
  };
  scroller.addEventListener(
    "scroll",
    function () {
      if (!row) return;
      var line = lineAt();
      if (line !== term.buffer.active.viewportY) term.scrollToLine(line);
    },
    { passive: true },
  );
  term.onScroll(sync);
  term.onWriteParsed(sync);
  // A full-screen program has the screen to itself, and its own touches: the
  // app keeps a drag there from swiping its sheet away.
  term.buffer.onBufferChange(function () {
    sync();
    post({ t: "full", on: term.buffer.active.type === "alternate" });
  });

  var refit = function () {
    try {
      fit.fit();
    } catch (error) {}
    var screen = document.querySelector(".xterm-screen");
    if (screen && term.rows) row = screen.getBoundingClientRect().height / term.rows;
    placeOverhang();
    sync();
    report();
  };
  var pending = 0;
  new ResizeObserver(function () {
    cancelAnimationFrame(pending);
    pending = requestAnimationFrame(refit);
  }).observe(document.body);

  // WebKit scrolls the page to keep the focused input in view as the keyboard
  // opens; the page always fits the view, so put it back.
  window.addEventListener("scroll", function () {
    if (window.scrollX || window.scrollY) window.scrollTo(0, 0);
  });

  // A tap anywhere brings up the keyboard: iOS only shows it for a tap.
  document.body.addEventListener("click", function () {
    term.focus();
  });

  window.__mono = {
    write: function (data, reset) {
      if (reset) term.reset();
      if (data) term.write(fromBase64(data));
    },
    focus: function () {
      term.focus();
    },
    look: function (next) {
      paint(next);
      term.options.theme = themeFor(next);
      placeOverhang();
      drawOverhang();
    },
    // How much of the page's bottom the key bar covers.
    inset: function (px) {
      if (px === inset) return;
      inset = px;
      document.getElementById("terminal").style.bottom = px + "px";
      refit();
    },
    blur: function () {
      term.blur();
    },
    // Bracketed when the program asked for it, so a pasted script is not run
    // line by line as it arrives.
    paste: function (data) {
      term.paste(new TextDecoder().decode(fromBase64(data)));
    },
  };
  refit();
  post({ t: "ready" });
})();
