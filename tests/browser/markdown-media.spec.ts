import { expect, test } from "@playwright/test";

test("markdown images load, enlarge, and close", async ({ page }) => {
  await page.goto("/tests/browser/media.html");
  const inbox = page.getByTestId("inbox");
  const chat = page.getByTestId("chat");

  // GitHub release and blob screenshots render; other hosts stay a link.
  await expect(
    inbox.getByRole("img", { name: "Previous timer" }),
  ).toBeVisible();
  await expect(inbox.getByRole("img", { name: "New timer" })).toBeVisible();
  await expect(
    inbox.getByRole("link", { name: "Hosted elsewhere" }),
  ).toBeVisible();
  // An issue body never reads files from this machine.
  await expect(inbox.getByRole("img", { name: "Local file" })).toHaveCount(0);
  // Width caps keep side-by-side screenshots inside the table.
  const scroller = inbox
    .locator('[data-streamdown="table-wrapper"] > div')
    .first();
  expect(
    await scroller.evaluate((el) => el.scrollWidth - el.clientWidth),
  ).toBeLessThanOrEqual(1);

  // An agent's screenshot path renders from disk.
  await expect(
    chat.getByRole("img", { name: "Settings screen" }),
  ).toBeVisible();
  await expect(chat.getByText("missing.png (image unavailable)")).toBeVisible();

  await chat.getByRole("button", { name: "Enlarge Settings screen" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await page.mouse.click(5, 5);
  await expect(dialog).toHaveCount(0);

  await inbox.getByRole("button", { name: "Enlarge before" }).click();
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
});
