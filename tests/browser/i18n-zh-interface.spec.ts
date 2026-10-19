import { expect, test } from "@playwright/test";

/**
 * Guards the Simplified Chinese interface: the shell rail, the Inbox and Notes
 * surfaces, and the keybindings table must render Chinese, not the English
 * source strings they are keyed by.
 */
test.use({ viewport: { width: 1400, height: 950 } });

test("the zh-CN interface covers the shell, Inbox, Notes and keybindings", async ({
  page,
}) => {
  test.setTimeout(120_000);

  await page.addInitScript(() => {
    localStorage.setItem("monocode.locale", "zh-CN");
    localStorage.setItem("monocode.insetLayout", "1");
  });

  await page.goto("/tests/browser/workspace-performance.html");
  await page.locator(".inset-shell").waitFor();

  await page.evaluate(() => {
    const fixture = window as unknown as {
      __TAURI_INTERNALS__: {
        invoke: (
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<unknown>;
      };
    };
    const base = fixture.__TAURI_INTERNALS__.invoke;
    fixture.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (
        [
          "notes_list",
          "automations_list",
          "automation_runs_list",
          "automation_runs_recover",
          "inbox_list",
        ].includes(command)
      ) {
        return Promise.resolve([]);
      }
      return base(command, args);
    };
  });

  // Shell rail and sidebar expose translated view labels. The harness page
  // boots the shell without main.tsx, so assert on rendered copy rather than
  // the <html lang> attribute.
  const shell = page.locator("body");
  await expect(shell.getByText("收件箱", { exact: true }).first()).toBeVisible();
  await expect(shell.getByText("笔记", { exact: true }).first()).toBeVisible();

  // No English source label survives for those two views.
  await expect(
    shell.getByText("Inbox", { exact: true }),
  ).toHaveCount(0);

  // Settings → Keybindings renders Chinese column headers and command names.
  await page.keyboard.press("Meta+,");
  const keybindingsNav = page.getByText("快捷键", { exact: true }).first();
  await keybindingsNav.waitFor({ timeout: 15_000 });
  await keybindingsNav.click();
  await expect(page.getByText("命令", { exact: true }).first()).toBeVisible();
  await expect(page.getByText("应用：设置", { exact: true })).toBeVisible();

  await page.screenshot({
    path: "test-results/zh-interface-shell.png",
    fullPage: true,
  });
});
