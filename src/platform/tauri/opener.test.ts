// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("../../features/browser/model/openLink", () => ({ openLink: vi.fn() }));

import { terminalLinkClick } from "./opener";

describe("terminalLinkClick", () => {
  it("recognizes clicks inside a terminal only", () => {
    const terminal = document.createElement("div");
    terminal.className = "xterm";
    const cell = document.createElement("span");
    terminal.append(cell);
    const elsewhere = document.createElement("button");
    document.body.append(terminal, elsewhere);

    const inside = new MouseEvent("click", { bubbles: true });
    cell.dispatchEvent(inside);
    expect(terminalLinkClick(inside)).toBe(inside);

    const outside = new MouseEvent("click", { bubbles: true });
    elsewhere.dispatchEvent(outside);
    expect(terminalLinkClick(outside)).toBeNull();

    expect(terminalLinkClick(new KeyboardEvent("keydown"))).toBeNull();
    expect(terminalLinkClick(undefined)).toBeNull();
  });
});
