// @vitest-environment happy-dom
import { expect, it } from "vitest";
import { sonarExampleDiffs, sonarHtmlToMarkdown } from "./sonarRuleContent";
it("preserves explanation, fix examples and links without executable HTML", () => {
  const markdown = sonarHtmlToMarkdown(
    '<h2>Why is this an issue?</h2><p>Validate <code>ModelState.IsValid</code>.</p><pre>Task&lt;bool&gt; Check()\n{ return true; }</pre><a href="https://example.com/fix">Fix details</a><script>alert(1)</script><a href="javascript:alert(1)">Unsafe</a>',
  );
  expect(markdown).toContain("## Why is this an issue?");
  expect(markdown).toContain("`ModelState.IsValid`");
  expect(markdown).toContain("Task<bool> Check()");
  expect(markdown).toContain("[Fix details](https://example.com/fix)");
  expect(markdown).not.toContain("javascript:");
  expect(markdown).not.toContain("<script>");
});
it("pairs the matching compliant and noncompliant examples for a real example diff", () => {
  const examples = sonarExampleDiffs(
    '<pre data-diff-id="1" data-diff-type="compliant">if (ModelState.IsValid) Save();</pre><pre data-diff-id="2" data-diff-type="noncompliant">unpaired</pre><pre data-diff-id="1" data-diff-type="noncompliant">Save();</pre>',
  );
  expect(examples).toEqual([
    {
      id: "1",
      original: "Save();",
      current: "if (ModelState.IsValid) Save();",
    },
  ]);
});
