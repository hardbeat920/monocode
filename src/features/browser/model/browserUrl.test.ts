import { describe, expect, it } from "vitest";
import {
  browserUrlFromInput,
  browserUrlLabel,
  isBrowsableUrl,
} from "./browserUrl";

describe("browserUrlFromInput", () => {
  it("keeps full web URLs", () => {
    expect(browserUrlFromInput("https://example.com/a?b=1")).toBe(
      "https://example.com/a?b=1",
    );
    expect(browserUrlFromInput("http://example.com")).toBe(
      "http://example.com/",
    );
  });

  it("adds https to bare hosts and http to local dev servers", () => {
    expect(browserUrlFromInput("example.com")).toBe("https://example.com/");
    expect(browserUrlFromInput("docs.rs/tauri")).toBe("https://docs.rs/tauri");
    expect(browserUrlFromInput("localhost:3000")).toBe(
      "http://localhost:3000/",
    );
    expect(browserUrlFromInput("127.0.0.1:5173/app")).toBe(
      "http://127.0.0.1:5173/app",
    );
    expect(browserUrlFromInput("devbox:8080")).toBe("https://devbox:8080/");
  });

  it("searches for everything else", () => {
    expect(browserUrlFromInput("tauri child webview")).toBe(
      "https://duckduckgo.com/?q=tauri%20child%20webview",
    );
    expect(browserUrlFromInput("javascript:alert(1)")).toContain(
      "duckduckgo.com",
    );
    expect(browserUrlFromInput("  ")).toBe("about:blank");
  });
});

describe("isBrowsableUrl", () => {
  it("allows only web pages", () => {
    expect(isBrowsableUrl("https://example.com")).toBe(true);
    expect(isBrowsableUrl("about:blank")).toBe(true);
    expect(isBrowsableUrl("file:///etc/hosts")).toBe(false);
    expect(isBrowsableUrl("mailto:a@b.c")).toBe(false);
  });
});

describe("browserUrlLabel", () => {
  it("shows the host", () => {
    expect(browserUrlLabel("https://example.com/a")).toBe("example.com");
    expect(browserUrlLabel("about:blank")).toBe("New Tab");
  });
});
