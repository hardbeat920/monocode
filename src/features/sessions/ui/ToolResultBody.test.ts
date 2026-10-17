// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("./HighlightedCodeBlock", () => ({
  HighlightedCodeBlock: ({
    code,
    isIncomplete,
  }: {
    code: string;
    isIncomplete: boolean;
  }) =>
    createElement(
      "pre",
      { "data-incomplete-json": String(isIncomplete) },
      code,
    ),
}));

import { ToolResultBody } from "./ToolResultBody";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

it("indents complete JSON only while the disclosure is mounted", () => {
  act(() =>
    root.render(
      createElement(ToolResultBody, {
        output: '{"ok":true,"items":[{"name":"one"}]}',
        id: "result-1",
      }),
    ),
  );
  const region = container.querySelector<HTMLElement>('[role="region"]')!;
  expect(region.id).toBe("result-1");
  expect(region.getAttribute("aria-label")).toBe("Tool result");
  expect(region.querySelector("pre")?.textContent).toContain('\n  "items": [');
  expect(
    region
      .querySelector("[data-incomplete-json]")
      ?.getAttribute("data-incomplete-json"),
  ).toBe("false");
});

it.each([
  '[INFO] build step started\n[WARN] waiting for dependency\n…',
  '{ordinary console output}\n  preserve indentation\n…',
  '{"results":[{"name":"unfinished"}],"more":{"pending":\n…',
])("preserves malformed or truncated text verbatim: %s", (output) => {
  act(() => root.render(createElement(ToolResultBody, { output })));
  expect(container.querySelector("[data-tool-result-text]")?.textContent).toBe(output);
  expect(container.querySelector("[data-incomplete-json]")).toBeNull();
});

it("preserves stdout and other invalid JSON as escaped, scrollable text", () => {
  const output =
    '<script>unsafe</script>\n{"unfinished":…\n' + "line\n".repeat(200);
  act(() => root.render(createElement(ToolResultBody, { output })));
  const region = container.querySelector<HTMLElement>("[data-tool-result]")!;
  expect(region.className).toContain("max-h-64");
  expect(region.className).toContain("overflow-y-auto");
  expect(region.className).toContain("overscroll-y-auto");
  expect(region.querySelector("pre")?.textContent).toBe(output);
  expect(region.querySelector("script")).toBeNull();
  expect(region.querySelector("[data-incomplete-json]")).toBeNull();
});

it("shows command output first with status and workspace, and keeps raw JSON optional", () => {
  const output = JSON.stringify({
    command_line: "git status",
    workingDir: "D:\\primewire-desktop",
    exitCode: 1,
    exit_code: 1,
    combined_output: "fatal: not a git repository\n",
    formatted_output: "fatal: not a git repository\n",
  });
  act(() => root.render(createElement(ToolResultBody, { output })));
  const region = container.querySelector<HTMLElement>("[data-tool-result]")!;
  expect(region.textContent).toContain("git status");
  expect(region.textContent).toContain("D:\\primewire-desktop");
  expect(region.textContent).toContain("Exit 1");
  expect(region.querySelector("pre")?.textContent).toBe(
    "fatal: not a git repository\n",
  );
  expect(region.textContent).not.toContain("combined_output");
  const raw = Array.from(region.querySelectorAll("button")).find(
    (element) => element.textContent === "View raw JSON",
  )!;
  act(() => raw.click());
  expect(region.querySelector("[data-tool-result-raw]")?.textContent).toContain(
    '"combined_output"',
  );
  expect(region.querySelector("button")?.getAttribute("aria-expanded")).toBe(
    "true",
  );
  act(() => region.querySelector<HTMLButtonElement>("button")!.click());
  expect(region.querySelector("[data-tool-result-raw]")).toBeNull();
});

it("shows each result from a grouped command call without duplicating raw output", () => {
  const output = JSON.stringify({
    results: [
      { command: "pwd", stdout: "D:\\repo", stderr: "", exitCode: 0 },
      { command: "npm test", stdout: "1 passed", stderr: "", exitCode: 0 },
    ],
  });
  act(() => root.render(createElement(ToolResultBody, { output })));
  const region = container.querySelector<HTMLElement>("[data-tool-result]")!;
  expect(region.querySelectorAll("pre")).toHaveLength(2);
  expect(region.textContent).toContain("pwd");
  expect(region.textContent).toContain("npm test");
  expect(region.textContent).toContain("1 passed");
  expect(region.textContent).not.toContain('"stdout"');
});

it("recognizes an 8k-capped commandLine/workingDir/exitCode/combinedOutput envelope instead of rendering giant escaped JSON", () => {
  const raw = JSON.stringify({
    commandLine: "npm run tests",
    workingDir: "D:\\primewire-desktop",
    exitCode: 0,
    combinedOutput: "Tests passed\n" + "line of output\n".repeat(1200),
  });
  const output = raw.slice(0, 8_000) + "\n…";
  act(() => root.render(createElement(ToolResultBody, { output })));
  const region = container.querySelector<HTMLElement>("[data-tool-result]")!;
  expect(region.textContent).toContain("npm run tests");
  expect(region.textContent).toContain("D:\\primewire-desktop");
  expect(region.textContent).toContain("Exit 0");
  expect(region.querySelector("pre")?.textContent).toContain("Tests passed\nline of output\n");
  expect(region.querySelector("pre")?.textContent).not.toContain('\\nline of output\\n');
  expect(region.textContent).not.toContain('"combinedOutput"');
  expect(region.textContent).toContain("Output truncated");
  const rawButton = Array.from(region.querySelectorAll("button")).find((button) => button.textContent === "View raw JSON");
  expect(rawButton).toBeDefined();
  act(() => rawButton?.click());
  expect(region.querySelector("[data-tool-result-raw]")?.textContent).toContain('"combinedOutput"');
});

it("retains completed commands in an 8k-capped grouped result with the final output string unfinished", () => {
  const raw = JSON.stringify({
    results: [
      { commandLine: "pwd", workingDir: "D:\\repo", exitCode: 0, combinedOutput: "D:\\repo\n" },
      { commandLine: "npm test", workingDir: "D:\\repo", exitCode: 1, combinedOutput: "fatal\n" + "failure text\n".repeat(1000) },
    ],
  });
  const output = raw.slice(0, 8_000) + "\n…";
  act(() => root.render(createElement(ToolResultBody, { output })));
  const region = container.querySelector<HTMLElement>("[data-tool-result]")!;
  expect(region.textContent).toContain("pwd");
  expect(region.textContent).toContain("npm test");
  expect(region.textContent).toContain("Exit 1");
  expect(region.querySelectorAll("pre").length).toBe(2);
  expect(region.querySelectorAll("pre")[1].textContent).toContain("fatal\nfailure text\n");
  expect(region.textContent).not.toContain('"combinedOutput"');
});

it("keeps PowerShell columns intact and horizontally scrollable without widening the transcript", () => {
  const listing = "Mode                LastWriteTime         Length Name\n" +
    "----                -------------         ------ ----\n" +
    "-a----              10/16/2026           1234   " + "VeryLongFilename".repeat(30);
  const output = JSON.stringify({ commandLine: "Get-ChildItem", workingDir: "C:\\repo", exitCode: 0, combinedOutput: listing });
  act(() => root.render(createElement(ToolResultBody, { output })));
  const region = container.querySelector<HTMLElement>("[data-tool-result]")!;
  const pre = region.querySelector("pre")!;
  expect(pre.textContent).toBe(listing);
  expect(pre.className).toContain("whitespace-pre");
  expect(pre.className).toContain("overflow-x-auto");
  expect(pre.className).not.toContain("break-words");
  expect(region.className).toContain("max-h-64");
});

it("discloses stdout-only ACP shell results without treating arbitrary JSON source as shell output", () => {
  act(() => root.render(createElement(ToolResultBody, { output: '{"stdout":"Done\\n"}' })));
  let region = container.querySelector<HTMLElement>("[data-tool-result]")!;
  expect(region.querySelector("pre")?.textContent).toBe("Done\n");
  expect(region.textContent).not.toContain('"stdout"');
  expect(region.querySelector("button")?.textContent).toBe("View raw JSON");
  act(() => root.render(createElement(ToolResultBody, { output: '{"ok":true,"message":"Done"}' })));
  region = container.querySelector<HTMLElement>("[data-tool-result]")!;
  expect(region.textContent).toContain('"ok": true');
  expect(region.querySelector("button")).toBeNull();
});

it("keeps an ACP array of completed command objects compact instead of showing raw fields", () => {
  const output = JSON.stringify([
    { commandLine: "pwd", stdout: "C:\\repo", exitCode: 0 },
    { commandLine: "git status", stdOut: "clean", stdErr: "", exitCode: 0 },
  ]);
  act(() => root.render(createElement(ToolResultBody, { output })));
  const region = container.querySelector<HTMLElement>("[data-tool-result]")!;
  expect(region.querySelectorAll("pre")).toHaveLength(2);
  expect(region.textContent).toContain("git status");
  expect(region.querySelectorAll("pre")[1]?.textContent).toBe("clean");
  expect(region.textContent).not.toContain('"commandLine"');
});

it("presents OpenCode Find text with its actual title, match count, and readable unwrapped lines", () => {
  const output =
    "Found 47 matches\nD:\\primewire-desktop\\src\\main.ts\n  Line 7937: TODO: keep this column\n  Line 8010: FIXME: fix";
  act(() => root.render(createElement(ToolResultBody, {
    output, kind: "search", title: "Find TODO|FIXME|HACK|XXX",
  })));
  const region = container.querySelector<HTMLElement>("[data-tool-result]")!;
  expect(region.querySelector("[data-tool-result-title]")?.textContent).toBe("Find TODO|FIXME|HACK|XXX");
  expect(region.querySelector("[data-tool-result-summary]")?.textContent).toBe("Found 47 matches");
  const lines = region.querySelector<HTMLElement>("[data-tool-result-text]")!;
  expect(lines.textContent).toContain("Line 7937: TODO: keep this column");
  expect(lines.className).toContain("whitespace-pre");
  expect(lines.className).toContain("overflow-x-auto");
  expect(region.textContent).not.toContain("Exit 0");
  expect(region.textContent).not.toContain("View raw JSON");
  expect(lines.textContent).not.toContain("Found 47 matches");
});

it("uses the same compact frame for OpenCode read and plain shell output without inventing metadata", () => {
  act(() => root.render(createElement(ToolResultBody, {
    output: "1\tconst answer = 42;\n2\tconsole.log(answer);",
    kind: "read", title: "Read src/main.ts",
  })));
  let region = container.querySelector<HTMLElement>("[data-tool-result]")!;
  expect(region.querySelector("[data-tool-result-title]")?.textContent).toBe("Read src/main.ts");
  expect(region.querySelector("[data-tool-result-text]")?.textContent).toContain("console.log(answer)");
  expect(region.textContent).not.toContain("Exit ");
  act(() => root.render(createElement(ToolResultBody, {
    output: "On branch main\nnothing to commit", title: "git status",
  })));
  region = container.querySelector<HTMLElement>("[data-tool-result]")!;
  expect(region.querySelector("[data-tool-result-title]")?.textContent).toBe("git status");
  expect(region.querySelector("[data-tool-result-text]")?.textContent).toContain("nothing to commit");
  expect(region.textContent).not.toContain("Exit ");
});

it("never treats a search/read JSON result with stdout fields as a shell execution", () => {
  const output = '{"stdout":"literal JSON field","exitCode":7}';
  act(() => root.render(createElement(ToolResultBody, {
    output, kind: "read", title: "Read fixture.json",
  })));
  const region = container.querySelector<HTMLElement>("[data-tool-result]")!;
  expect(region.querySelector("[data-tool-result-title]")?.textContent).toBe("Read fixture.json");
  expect(region.querySelector("pre")?.textContent).toContain('"stdout": "literal JSON field"');
  expect(region.textContent).not.toContain("Exit 7");
  expect(region.textContent).not.toContain("View raw JSON");
});

it.each([false, true])("hands edge scrolling to the nearest parent (activity=%s)", (inActivity) => {
  const transcript = document.createElement("div");
  transcript.className = "agent-transcript";
  const activity = document.createElement("div");
  activity.className = "zen-phase-live";
  document.body.append(transcript);
  if (inActivity) transcript.append(activity);
  (inActivity ? activity : transcript).append(container);
  for (const el of [transcript, activity]) {
    Object.defineProperties(el, {
      scrollHeight: { value: 500 },
      clientHeight: { value: 100 },
    });
    el.scrollTop = 100;
  }
  try {
    act(() => root.render(createElement(ToolResultBody, { output: "Short output" })));
    const region = container.querySelector<HTMLElement>("[data-tool-result]")!;
    const wheel = new WheelEvent("wheel", { deltaY: -30, bubbles: true, cancelable: true });
    region.dispatchEvent(wheel);
    expect(wheel.defaultPrevented).toBe(true);
    expect(transcript.scrollTop).toBe(inActivity ? 100 : 70);
    expect(activity.scrollTop).toBe(inActivity ? 70 : 100);
    region.dispatchEvent(new WheelEvent("wheel", { deltaY: 30, bubbles: true, cancelable: true }));
    expect(transcript.scrollTop).toBe(100);
    expect(activity.scrollTop).toBe(100);
  } finally {
    document.body.append(container);
    transcript.remove();
  }
});
