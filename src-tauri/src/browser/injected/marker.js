// MonoCode tab marker (spec 7, decision D12).
//
// Runs in the isolated world "__monocode" of the main frame of every tab the
// agent opened, registered once per tab with
// Page.addScriptToEvaluateOnNewDocument { worldName: "__monocode" }, so it is
// in place before the page's own scripts on every new document.
//
// It prefixes document.title with "◆ MonoCode · " and swaps the favicon for a
// MonoCode diamond, then keeps the prefix in place when the page rewrites its
// title (at most 10 reapplications per second). Snapshots and results strip
// the prefix again on the Rust side (protocol::strip_marker), so the agent
// never sees it. Nothing here reads page content or talks to the page.
(() => {
  const PREFIX = "◆ MonoCode · ";
  const BARE = "◆ MonoCode";
  const ICON =
    "data:image/svg+xml," +
    encodeURIComponent(
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">' +
        '<path d="M8 1 15 8 8 15 1 8Z" fill="#1f1f1f"/>' +
        '<path d="M8 4 12 8 8 12 4 8Z" fill="#ffffff"/>' +
        "</svg>"
    );
  const protocol = String(location.protocol || "");
  // Marker does not run on internal pages (spec 7); we never navigate there.
  if (
    protocol === "chrome:" ||
    protocol === "edge:" ||
    protocol === "about:" ||
    protocol === "file:" ||
    protocol === "devtools:" ||
    protocol === "chrome-extension:"
  ) {
    return;
  }
  if (window.top !== window) {
    return;
  }

  let applications = 0;
  let windowStart = 0;
  let applying = false;
  let released = false;
  // Original favicon links, detached while ours is shown and restored when
  // the user takes the tab back ("Preuzmi tab", spec 13.1).
  const originalIcons = [];

  const budgetOk = () => {
    const now = Date.now();
    if (now - windowStart >= 1000) {
      windowStart = now;
      applications = 0;
    }
    applications += 1;
    return applications <= 10;
  };

  const applyTitle = () => {
    if (released || applying || !document.head) {
      return;
    }
    const current = document.title || "";
    if (current === BARE || current.startsWith(PREFIX)) {
      return;
    }
    if (!budgetOk()) {
      return;
    }
    applying = true;
    try {
      document.title = current.trim() ? PREFIX + current : BARE;
    } finally {
      applying = false;
    }
  };

  const applyIcon = () => {
    if (released || !document.head) {
      return;
    }
    const links = Array.from(document.head.querySelectorAll('link[rel~="icon"]'));
    let ours = links.find((l) => l.dataset.monocodeMarker === "1");
    for (const link of links) {
      if (link !== ours) {
        originalIcons.push(link);
        link.remove();
      }
    }
    if (!ours) {
      ours = document.createElement("link");
      ours.rel = "icon";
      ours.type = "image/svg+xml";
      ours.dataset.monocodeMarker = "1";
      document.head.appendChild(ours);
    }
    if (ours.href !== ICON) {
      ours.href = ICON;
    }
  };

  let observer = null;
  const observe = () => {
    if (observer || !document.documentElement) {
      return;
    }
    observer = new MutationObserver((records) => {
      let title = false;
      let icon = false;
      for (const record of records) {
        const target = record.target;
        if (
          target.nodeName === "TITLE" ||
          (target.parentNode && target.parentNode.nodeName === "TITLE") ||
          Array.from(record.addedNodes).some((n) => n.nodeName === "TITLE")
        ) {
          title = true;
        }
        if (
          target.nodeName === "LINK" ||
          Array.from(record.addedNodes).some((n) => n.nodeName === "LINK") ||
          Array.from(record.removedNodes).some((n) => n.nodeName === "LINK")
        ) {
          icon = true;
        }
      }
      if (title) {
        applyTitle();
      }
      if (icon) {
        applyIcon();
      }
    });
    observer.observe(document.documentElement, {
      subtree: true,
      childList: true,
      characterData: true,
      attributes: true,
      attributeFilter: ["href", "rel"],
    });
  };

  const run = () => {
    if (released) {
      return;
    }
    observe();
    applyTitle();
    applyIcon();
  };

  // Called by the module through Runtime.callFunctionOn in this same
  // isolated world when the lease is released: stop observing, strip the
  // prefix and put the page's own favicon back. Idempotent.
  globalThis.__monocodeMarkerRelease = () => {
    if (released) {
      return true;
    }
    released = true;
    if (observer) {
      observer.disconnect();
      observer = null;
    }
    const current = document.title || "";
    if (current === BARE) {
      document.title = "";
    } else if (current.startsWith(PREFIX)) {
      document.title = current.slice(PREFIX.length);
    }
    if (document.head) {
      for (const link of Array.from(
        document.head.querySelectorAll('link[rel~="icon"][data-monocode-marker="1"]')
      )) {
        link.remove();
      }
      for (const link of originalIcons) {
        if (!link.isConnected) {
          document.head.appendChild(link);
        }
      }
    }
    originalIcons.length = 0;
    return true;
  };

  if (document.head) {
    run();
  } else {
    document.addEventListener("DOMContentLoaded", run, { once: true });
    // Head usually exists well before DOMContentLoaded; poll briefly so the
    // marker shows up as early as possible.
    let tries = 0;
    const poll = setInterval(() => {
      tries += 1;
      if (document.head) {
        clearInterval(poll);
        run();
      } else if (tries > 200) {
        clearInterval(poll);
      }
    }, 25);
  }
  window.addEventListener("load", () => {
    applyTitle();
    applyIcon();
  });
})();
