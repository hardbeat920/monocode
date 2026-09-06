import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { ProviderModelSelect } from "./ProviderModelSelect";
import type { AgentModel } from "../lib/models";

const models: AgentModel[] = [
  { id: "codex:gpt-6-astra", harness: "codex", name: "GPT-6-Astra" },
  { id: "codex:gpt-5.6-luna", harness: "codex", name: "GPT-5.6-Luna" },
  { id: "codex:gpt-5.6-sol", harness: "codex", name: "GPT-5.6-Sol" },
];

describe("ProviderModelSelect", () => {
  it("renders a harness-styled trigger for the selected model", () => {
    const html = renderToStaticMarkup(
      createElement(ProviderModelSelect, {
        label: "Codex model",
        harness: "codex",
        value: "codex:gpt-6-astra",
        models,
        onChange: vi.fn(),
      }),
    );

    expect(html).toContain("GPT-6-Astra");
    expect(html).toContain('aria-label="Codex model"');
    expect(html).toContain('aria-haspopup="listbox"');
    expect(html).toContain('aria-expanded="false"');
    expect(html).not.toContain("Search models...");
    expect(html).not.toContain("GPT-5.6-Luna");
    expect(html).not.toContain("Show free OpenCode models");
  });
});
