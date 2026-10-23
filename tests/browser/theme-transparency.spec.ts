import { expect, test } from "@playwright/test";

test.use({ viewport: { width: 1300, height: 900 } });

test("keeps the window opaque when its workspace theme or color scheme changes", async ({
  page,
}) => {
  test.setTimeout(60_000);
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "platform", {
      value: "MacIntel",
      configurable: true,
    });
    localStorage.setItem("monocode.insetLayout", "1");
    localStorage.setItem("monocode.colorScheme", "dark");
  });
  await page.goto("/tests/browser/workspace-performance.html");
  await expect(page.locator(".inset-shell")).toBeVisible({ timeout: 15_000 });
  await page.evaluate(async () => {
    const fixture = window as unknown as {
      nativeGlassChanges: boolean[];
      __TAURI_INTERNALS__: {
        invoke: (
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<unknown>;
      };
    };
    fixture.nativeGlassChanges = [];
    const base = fixture.__TAURI_INTERNALS__.invoke;
    fixture.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "set_window_glass_enabled") {
        fixture.nativeGlassChanges.push(Boolean(args?.enabled));
        return true;
      }
      return base(command, args);
    };
    // The workspace fixture skips the native boot splash; activate its real
    // appearance controller so theme changes exercise the native glass path.
    const path = "/src/features/settings/model/appearance.ts";
    const appearance = await import(path);
    appearance.initAppearance();
    appearance.activateWindowAppearance();
  });
  const root = page.locator("html");
  await expect(root).toHaveClass(/has-native-glass/);
  const modifier = await page.evaluate(() =>
    /Mac|iPhone|iPad/.test(navigator.platform) ? "Meta" : "Control",
  );
  await page.keyboard.press(`${modifier}+,`);
  await page.getByRole("button", { name: "Appearance", exact: true }).click();
  const transparent = page.getByRole("switch", {
    name: "Transparent window",
    exact: true,
  });
  await expect(transparent).toBeChecked();
  await transparent.click();
  await expect(transparent).not.toBeChecked();
  await expect(root).not.toHaveClass(/has-native-glass/);
  await expect
    .poll(() =>
      page.evaluate(() =>
        (
          window as unknown as { nativeGlassChanges: boolean[] }
        ).nativeGlassChanges.at(-1),
      ),
    )
    .toBe(false);

  for (const theme of ["Moss", "Ember"]) {
    await page.getByRole("button", { name: /^Workspace theme:/ }).click();
    await page.getByRole("option", { name: theme, exact: true }).click();
    await expect(transparent).not.toBeChecked();
    await expect(root).not.toHaveClass(/has-native-glass/);
    expect(
      await page.evaluate(() => localStorage.getItem("monocode.insetGlass")),
    ).toBe("0");
    await expect
      .poll(() =>
        page.evaluate(() =>
          (
            window as unknown as { nativeGlassChanges: boolean[] }
          ).nativeGlassChanges.at(-1),
        ),
      )
      .toBe(false);
  }
  const scheme = page.getByRole("radiogroup", { name: "Theme", exact: true });
  await scheme.getByRole("radio", { name: "Light", exact: true }).click();
  await expect(transparent).toBeDisabled();
  await expect(transparent).not.toBeChecked();
  await scheme.getByRole("radio", { name: "Dark", exact: true }).click();
  await expect(transparent).toBeEnabled();
  await expect(transparent).not.toBeChecked();
  await expect(root).not.toHaveClass(/has-native-glass/);

  await transparent.click();
  await expect(transparent).toBeChecked();
  await expect(root).toHaveClass(/has-native-glass/);
  await expect
    .poll(() =>
      page.evaluate(() =>
        (
          window as unknown as { nativeGlassChanges: boolean[] }
        ).nativeGlassChanges.at(-1),
      ),
    )
    .toBe(true);
});
