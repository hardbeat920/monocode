import { describe, expect, it } from "vitest";
import { normalizeBrowserUrl } from "./browserUrl";

describe("normalizeBrowserUrl", () => {
  it("keeps http and https URLs", () => {
    expect(normalizeBrowserUrl("https://example.com/a?b=1")).toBe(
      "https://example.com/a?b=1",
    );
    expect(normalizeBrowserUrl("http://example.com/")).toBe(
      "http://example.com/",
    );
  });

  it("trims surrounding whitespace", () => {
    expect(normalizeBrowserUrl("  https://example.com  ")).toBe(
      "https://example.com/",
    );
  });

  it("assumes https for a bare host", () => {
    expect(normalizeBrowserUrl("example.com")).toBe("https://example.com/");
    expect(normalizeBrowserUrl("example.com/docs")).toBe(
      "https://example.com/docs",
    );
  });

  it("assumes http for loopback and explicit ports", () => {
    expect(normalizeBrowserUrl("localhost:5173")).toBe(
      "http://localhost:5173/",
    );
    expect(normalizeBrowserUrl("127.0.0.1:8080/app")).toBe(
      "http://127.0.0.1:8080/app",
    );
    expect(normalizeBrowserUrl("localhost")).toBe("http://localhost/");
    expect(normalizeBrowserUrl("example.com:3000")).toBe(
      "http://example.com:3000/",
    );
  });

  it("rejects empty input", () => {
    expect(normalizeBrowserUrl("")).toBeNull();
    expect(normalizeBrowserUrl("   ")).toBeNull();
  });

  it("rejects schemes that could reach local files or run script", () => {
    expect(normalizeBrowserUrl("file:///etc/passwd")).toBeNull();
    expect(normalizeBrowserUrl("javascript:alert(1)")).toBeNull();
    expect(normalizeBrowserUrl("data:text/html,<h1>x</h1>")).toBeNull();
    expect(normalizeBrowserUrl("asset://localhost/x")).toBeNull();
  });
});
