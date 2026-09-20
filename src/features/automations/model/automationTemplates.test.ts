import { describe, expect, it } from "vitest";
import { SUPPORTED_INBOX_TRIGGER_EVENTS } from "./automationEvents";
import {
  AUTOMATION_TEMPLATE_CATEGORIES,
  AUTOMATION_TEMPLATES,
  templatesForCategory,
} from "./automationTemplates";

describe("automation templates", () => {
  it("keeps popular examples in their real category too", () => {
    const popular = templatesForCategory("popular");
    expect(popular.map((template) => template.id)).toEqual([
      "find-critical-bugs",
      "scan-vulnerabilities",
      "generate-docs",
      "add-test-coverage",
    ]);
    expect(templatesForCategory("review").some((t) => t.popular)).toBe(true);
    expect(templatesForCategory("security").some((t) => t.popular)).toBe(true);
  });

  it("covers every gallery category with at least one example", () => {
    for (const category of AUTOMATION_TEMPLATE_CATEGORIES) {
      expect(templatesForCategory(category.id).length).toBeGreaterThan(0);
    }
    expect(AUTOMATION_TEMPLATES.every((template) => template.prompt.trim())).toBe(
      true,
    );
  });

  it("only uses inbox events that actually fire", () => {
    for (const template of AUTOMATION_TEMPLATES) {
      if (template.trigger.kind === "time") continue;
      const supported =
        SUPPORTED_INBOX_TRIGGER_EVENTS[
          template.trigger.kind as keyof typeof SUPPORTED_INBOX_TRIGGER_EVENTS
        ];
      expect(supported).toContain(template.trigger.event);
    }
  });
});
