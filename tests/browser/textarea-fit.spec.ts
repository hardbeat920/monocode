import { expect, test } from "@playwright/test";

const draft = Array.from(
  { length: 3 },
  (_, line) =>
    `Line ${line}: a draft long enough to wrap once the pane narrows.`,
).join("\n");

for (const pane of ["composer", "instructions"]) {
  test(`${pane} refits its draft when the pane narrows`, async ({ page }) => {
    await page.goto("/tests/browser/textarea.html");
    const wrapper = page.locator(`[data-pane="${pane}"]`);
    const field = wrapper.locator("textarea");
    await field.fill(draft);
    // The draft fits under the cap, so the field hides its scrollbar.
    await expect(field).toHaveCSS("overflow-y", "hidden");

    await wrapper.evaluate((el) => (el.style.width = "260px"));
    // Every rewrapped line stays visible: the field grew to the text, or
    // reached its cap and scrolls.
    await expect
      .poll(() =>
        field.evaluate((el) => {
          const hidden = el.scrollHeight - el.clientHeight;
          return hidden <= 1 || getComputedStyle(el).overflowY === "auto";
        }),
      )
      .toBe(true);
  });
}
