import { expect, test } from "@playwright/test";

test.use({ viewport: { width: 1300, height: 900 } });

test("keeps many processes compact, with details and stop controls on demand", async ({
  page,
}, testInfo) => {
  test.setTimeout(60_000);
  await page.addInitScript(() =>
    localStorage.setItem("monocode.insetLayout", "1"),
  );
  await page.goto("/tests/browser/workspace-performance.html");
  await expect(page.locator(".inset-shell")).toBeVisible({ timeout: 15_000 });
  await page.evaluate(() => {
    const fixture = window as unknown as {
      resourceStops: string[];
      __TAURI_INTERNALS__: {
        invoke: (
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<unknown>;
      };
    };
    fixture.resourceStops = [];
    let rows = Array.from({ length: 60 }, (_, index) => ({
      pid: 100 + index,
      identity: `process:${index}`,
      sessionId: `workspace-session-${Math.floor(index / 10)}`,
      cwd: "/fixture/project",
      command:
        index % 10 === 0
          ? "claude"
          : `npm run build --workspace=packages/${"long-package-name-".repeat(12)}`,
      cpuPercent: 40,
      memoryBytes: 120 * 1024 * 1024,
      agent: index % 10 === 0,
    }));
    const base = fixture.__TAURI_INTERNALS__.invoke;
    fixture.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "resource_usage")
        return { cpuPercent: 95, memoryPressure: 2 };
      if (command === "resource_processes") return rows;
      if (command === "resource_stop_tool") {
        fixture.resourceStops.push(String(args?.identity));
        rows = rows.filter((row) => row.identity !== args?.identity);
        return;
      }
      return base(command, args);
    };
  });

  const footer = page.getByRole("contentinfo", { name: "Provider usage" });
  const chip = footer.getByRole("button", {
    name: "High resource usage",
    exact: true,
  });
  await expect(chip).toBeVisible({ timeout: 15_000 });
  await expect(chip).toHaveText("High usage");
  await chip.click();
  const dialog = page.getByRole("dialog", {
    name: "Running work",
    exact: true,
  });
  await expect(dialog.getByRole("listitem")).toHaveCount(6);
  await expect(
    dialog.getByRole("button", { name: /^Stop session/ }),
  ).toHaveCount(6);
  const group = dialog.locator('[data-resource-session="workspace-session-0"]');
  await expect(group.locator("code").first()).toBeHidden();
  const list = dialog.getByRole("list", { name: "Running sessions" });
  expect(await list.evaluate((el) => el.scrollHeight > el.clientHeight)).toBe(
    true,
  );
  const bounds = await dialog.boundingBox();
  expect(bounds!.width).toBeLessThanOrEqual(420);
  expect(bounds!.height).toBeLessThan(550);
  await page.screenshot({
    path: testInfo.outputPath("resource-pressure-dark.png"),
  });

  await group.locator("summary").click();
  await expect(group.locator("code").first()).toBeVisible();
  await group
    .getByRole("button", {
      name: "Stop build/tool for Session 1 (PID 101)",
      exact: true,
    })
    .click();
  await expect(group.locator("summary")).toContainText("9 processes");
  expect(
    await page.evaluate(
      () => (window as unknown as { resourceStops: string[] }).resourceStops,
    ),
  ).toEqual(["process:1"]);
  expect(
    await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth + 1),
  ).toBe(true);
  await page.screenshot({
    path: testInfo.outputPath("resource-pressure-details.png"),
  });
  await group.locator("summary").click();

  await page.evaluate(() =>
    document.documentElement.classList.add("theme-light"),
  );
  await page.screenshot({
    path: testInfo.outputPath("resource-pressure-light.png"),
  });
  await page.setViewportSize({ width: 360, height: 640 });
  const small = await dialog.boundingBox();
  expect(small!.width).toBeLessThanOrEqual(336);
  expect(small!.y).toBeGreaterThanOrEqual(0);
  expect(small!.y + small!.height).toBeLessThanOrEqual(640);
  expect(
    await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth + 1),
  ).toBe(true);
  await page.screenshot({
    path: testInfo.outputPath("resource-pressure-small.png"),
  });

  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(chip).toBeVisible();

  await page.setViewportSize({ width: 1300, height: 900 });
  const modifier = await page.evaluate(() =>
    /Mac|iPhone|iPad/.test(navigator.platform) ? "Meta" : "Control",
  );
  await page.keyboard.press(`${modifier}+,`);
  await expect(page.locator("[data-app-settings]")).toBeVisible();
  await expect(
    page.getByRole("button", { name: "High resource usage", exact: true }),
  ).toHaveCount(0);
  await expect(page.locator(".approval-toast")).toHaveCount(0);
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(chip).toBeVisible();
  await expect(dialog).toHaveCount(0);
  await expect(page.locator(".approval-toast")).toHaveCount(0);
});
