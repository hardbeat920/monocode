// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clickScript,
  consoleScript,
  readScript,
  typeScript,
  waitScript,
} from "./browserScripts";

// The native side runs each script as the body of an async function.
const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor as new (
  body: string,
) => () => Promise<unknown>;
const run = (script: string) => new AsyncFunction(script)();

beforeEach(() => {
  document.body.innerHTML = `
    <main id="app"><h1>Hello</h1><p>World</p>
      <form id="search"><input id="q" name="q" value="old" />
        <select id="size"><option value="s">Small</option><option value="l">Large</option></select>
        <button type="submit">Go</button>
      </form>
      <a href="#next" id="next">Next page</a>
    </main>`;
});

describe("page scripts", () => {
  it("reads text or HTML, whole page or one element, with truncation", async () => {
    const page = (await run(readScript({}))) as { content: string };
    expect(page.content).toContain("Hello");
    const html = (await run(readScript({ format: "html", selector: "h1" }))) as {
      content: string;
    };
    expect(html.content).toBe("<h1>Hello</h1>");
    const short = (await run(readScript({ selector: "main", maxChars: 3 }))) as {
      content: string;
      truncated: boolean;
    };
    expect(short).toMatchObject({ truncated: true });
    expect(short.content).toHaveLength(3);
    await expect(run(readScript({ selector: "#missing" }))).rejects.toThrow(
      "No element matches #missing",
    );
  });

  it("clicks by selector or by visible text", async () => {
    vi.useFakeTimers();
    const clicked = vi.fn();
    document.getElementById("next")!.addEventListener("click", clicked);
    const result = await run(clickScript({ text: "next page" }));
    expect(result).toEqual({ clicked: 'a#next "Next page"' });
    vi.runAllTimers();
    expect(clicked).toHaveBeenCalledTimes(1);
    await expect(run(clickScript({ selector: ".nope" }))).rejects.toThrow(
      "No element matches .nope",
    );
    vi.useRealTimers();
  });

  it("types into inputs and selects, firing input and change", async () => {
    const input = document.getElementById("q") as HTMLInputElement;
    const events: string[] = [];
    input.addEventListener("input", () => events.push("input"));
    input.addEventListener("change", () => events.push("change"));
    await run(typeScript({ selector: "#q", text: "new" }));
    expect(input.value).toBe("new");
    await run(typeScript({ selector: "#q", text: "er", clear: false }));
    expect(input.value).toBe("newer");
    expect(events).toEqual(["input", "change", "input", "change"]);

    await run(typeScript({ selector: "#size", text: "Large" }));
    expect((document.getElementById("size") as HTMLSelectElement).value).toBe("l");
    await expect(run(typeScript({ selector: "h1", text: "x" }))).rejects.toThrow(
      "is not editable",
    );
  });

  it("waits for elements and text", async () => {
    setTimeout(() => {
      document.body.insertAdjacentHTML("beforeend", "<div class='late'>Done!</div>");
    }, 50);
    await expect(run(waitScript({ selector: ".late", timeoutMs: 2000 }))).resolves.toMatchObject({
      found: true,
    });
    await expect(run(waitScript({ text: "Done!", timeoutMs: 500 }))).resolves.toMatchObject({
      found: true,
    });
    await expect(run(waitScript({ text: "never", timeoutMs: 150 }))).rejects.toThrow(
      "Timed out",
    );
  });

  it("reads and clears captured console entries", async () => {
    expect(await run(consoleScript({}))).toMatchObject({ entries: [] });
    (window as unknown as { __monocodeConsole: unknown[] }).__monocodeConsole = [
      { level: "log", text: "a" },
      { level: "error", text: "b" },
    ];
    expect(await run(consoleScript({ limit: 1, clear: true }))).toEqual({
      entries: [{ level: "error", text: "b" }],
      total: 2,
    });
    expect(await run(consoleScript({}))).toEqual({ entries: [], total: 0 });
  });

  it("embeds agent input as data, never as code", async () => {
    const hostile = '"); document.body.innerHTML = "pwned"; ("';
    await expect(run(readScript({ selector: hostile }))).rejects.toThrow();
    expect(document.body.innerHTML).not.toContain("pwned");
  });
});
