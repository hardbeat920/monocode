// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import { repairMermaid } from "./mermaidRepair";

describe("repairMermaid", () => {
  it("escapes semicolons inside sequence diagram messages and notes", () => {
    const source = [
      "sequenceDiagram",
      "    App->>DB: Load registration; check expiry",
      "    Note over App: one; two",
      "    App->>IIQ: Check account;",
    ].join("\n");

    expect(repairMermaid(source)).toBe(
      [
        "sequenceDiagram",
        "    App->>DB: Load registration#59; check expiry",
        "    Note over App: one#59; two",
        "    App->>IIQ: Check account;",
      ].join("\n"),
    );
  });

  it("leaves other diagram types alone", () => {
    const source = "flowchart LR\n  A[one; two] --> B";
    expect(repairMermaid(source)).toBe(source);
  });
});

describe("repairMermaid entities", () => {
  const mixed = [
    "sequenceDiagram",
    "    App->>DB: Load#59; then save; then #quot;done#quot;",
    "    Note over App: one#59; two; three",
  ].join("\n");

  it("keeps existing entities whole and escapes only bare semicolons", () => {
    expect(repairMermaid(mixed)).toBe(
      [
        "sequenceDiagram",
        "    App->>DB: Load#59; then save#59; then #quot;done#quot;",
        "    Note over App: one#59; two#59; three",
      ].join("\n"),
    );
  });

  it("changes nothing when run again", () => {
    const once = repairMermaid(mixed);
    expect(repairMermaid(once)).toBe(once);
  });

  it("parses in Mermaid with the labels intact", async () => {
    const { default: mermaid } = await import("mermaid");
    expect(await mermaid.parse(mixed, { suppressErrors: true })).toBe(false);

    const diagram = await mermaid.mermaidAPI.getDiagramFromText(
      repairMermaid(mixed),
    );
    const db = diagram.db as unknown as {
      getMessages(): Array<{ message: string }>;
    };
    expect(db.getMessages().map((entry) => decodeLabel(entry.message))).toEqual(
      ['Load; then save; then "done"', "one; two; three"],
    );
  });
});

/** Undoes Mermaid's placeholder encoding, then the HTML entities, as rendering does. */
function decodeLabel(text: string): string {
  const html = text
    .replace(/ﬂ°°/g, "&#")
    .replace(/ﬂ°/g, "&")
    .replace(/¶ß/g, ";");
  const element = document.createElement("textarea");
  element.innerHTML = html;
  return element.value;
}

describe("repairMermaid alphanumeric entities", () => {
  const source = "sequenceDiagram\n    A->>B: Use #frac12; cup; stir";

  it("keeps the entity whole, escapes the bare semicolon, and is idempotent", () => {
    const once = repairMermaid(source);

    expect(once).toBe("sequenceDiagram\n    A->>B: Use #frac12; cup#59; stir");
    expect(repairMermaid(once)).toBe(once);
  });

  it("parses in Mermaid with the label intact", async () => {
    const { default: mermaid } = await import("mermaid");
    const diagram = await mermaid.mermaidAPI.getDiagramFromText(
      repairMermaid(source),
    );
    const db = diagram.db as unknown as {
      getMessages(): Array<{ message: string }>;
    };

    expect(db.getMessages().map((entry) => decodeLabel(entry.message))).toEqual(
      ["Use ½ cup; stir"],
    );
  });
});
