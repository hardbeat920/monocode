import { expect, test, type Locator, type Page } from "@playwright/test";

const sessionPlus = (page: Page) =>
  page.getByRole("button", { name: "New session", exact: true });
const terminalPlus = (page: Page) =>
  page.getByRole("button", { name: /^New Terminal \(/ });
const field = (page: Page) => page.locator(".session-composer textarea");

async function box(locator: Locator) {
  const bounds = await locator.boundingBox();
  if (!bounds) throw new Error("Missing bounds");
  return bounds;
}

async function adjacent(tab: Locator, action: Locator) {
  const t = await box(tab);
  const a = await box(action);
  expect(a.x - (t.x + t.width)).toBeGreaterThanOrEqual(0);
  expect(a.x - (t.x + t.width)).toBeLessThanOrEqual(12);
  expect(Math.abs(a.y + a.height / 2 - (t.y + t.height / 2))).toBeLessThan(2);
}

for (const closed of [false, true]) {
  test(`plus follows fitting tabs, sidebar ${closed ? "closed" : "open"}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1120, height: 720 });
    await page.goto(`/tests/browser/ui-review.html?${closed ? "closed" : ""}`);
    await expect(sessionPlus(page)).toHaveCount(1);
    await expect(terminalPlus(page)).toHaveCount(1);
    await adjacent(
      page.locator("[data-title-tab-id]").last(),
      sessionPlus(page),
    );
    await adjacent(
      page
        .getByRole("tablist", { name: "Terminals" })
        .locator("[data-tab-slot-id]")
        .last(),
      terminalPlus(page),
    );
    const plus = await box(sessionPlus(page));
    expect(plus.x + plus.width).toBeLessThan(700);
    // Terminal focus must not redirect the session button to terminal creation.
    await page.getByRole("textbox", { name: "Terminal input" }).focus();
    await sessionPlus(page).click();
    await expect(page.locator("[data-created]")).toHaveText("1");
    await expect(page.locator("[data-terminal-created]")).toHaveText("0");
    await expect(field(page)).toBeFocused();
    await sessionPlus(page).focus();
    await page.keyboard.press("Enter");
    await expect(page.locator("[data-created]")).toHaveText("2");
  });
}

test("overflow keeps session plus reachable and scrolls the newly active tab into view", async ({
  page,
}) => {
  await page.setViewportSize({ width: 320, height: 720 });
  await page.goto("/tests/browser/ui-review.html?closed&overflow");
  const strip = page.locator("[data-title-tab-strip]");
  expect(await strip.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(
    true,
  );
  await expect(sessionPlus(page)).toBeInViewport({ ratio: 1 });
  await sessionPlus(page).click();
  await expect(page.locator("[data-created]")).toHaveText("1");
  const active = page.locator('[data-title-tab-id="new-1"]');
  await expect(active).toBeInViewport();
  await expect
    .poll(async () => {
      const s = await box(strip);
      const t = await box(active);
      return t.x >= s.x - 1 && t.x + t.width <= s.x + s.width + 1;
    })
    .toBe(true);
  await expect(sessionPlus(page)).toBeInViewport({ ratio: 1 });
});

for (const side of ["top", "bottom", "left", "right"]) {
  test(`terminal plus stays adjacent and reachable in the ${side} dock`, async ({
    page,
  }) => {
    await page.goto(`/tests/browser/ui-review.html?closed&side=${side}`);
    const tabs = page.getByRole("tablist", { name: "Terminals" });
    await adjacent(
      tabs.locator("[data-tab-slot-id]").last(),
      terminalPlus(page),
    );
    for (let count = 1; count <= 5; count++) {
      await terminalPlus(page).click();
      await expect(page.locator("[data-terminal-created]")).toHaveText(
        String(count),
      );
    }
    await page.setViewportSize({ width: 320, height: 720 });
    await expect(terminalPlus(page)).toBeInViewport({ ratio: 1 });
    await expect(
      page.getByRole("button", { name: "Move Terminal", exact: true }),
    ).toBeInViewport({ ratio: 1 });
    await expect(
      page.getByRole("button", { name: /^Hide Terminal/ }),
    ).toBeInViewport({ ratio: 1 });
    expect(await tabs.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(
      true,
    );
    await terminalPlus(page).click();
    await expect(page.locator("[data-terminal-created]")).toHaveText("6");
    await expect(tabs.getByRole("tab", { selected: true })).toBeInViewport();
    await page
      .getByRole("button", { name: "Move Terminal", exact: true })
      .click();
    await expect(
      page.getByRole("menu", { name: "Move terminal", exact: true }),
    ).toBeVisible();
    await page.getByRole("menuitemcheckbox", { name: "Dock Right" }).click();
    await expect(
      page.getByRole("separator", { name: "Resize terminal" }),
    ).toHaveAttribute("aria-orientation", "vertical");
  });
}

test("Mono retains the full title allocation beside window controls", async ({
  page,
}) => {
  await page.addInitScript(() =>
    Object.defineProperty(navigator, "platform", { value: "Win32" }),
  );
  await page.setViewportSize({ width: 800, height: 720 });
  await page.goto("/tests/browser/ui-review.html?mono&closed");
  const title = await box(page.locator("[data-mono-title]"));
  // The existing sidebar toggle and window controls are the only reserved space.
  const controls = await box(
    page.getByRole("button", { name: "Minimize window" }),
  );
  expect(title.width).toBeGreaterThan(600);
  expect(Math.abs(title.x + title.width - controls.x)).toBeLessThanOrEqual(1);
  await expect(sessionPlus(page)).toHaveCount(0);
});

test("long model and project labels keep all composer controls reachable at 320px", async ({
  page,
}) => {
  await page.setViewportSize({ width: 320, height: 720 });
  await page.goto("/tests/browser/ui-review.html?closed&long&project");
  const model = page.locator(".session-composer [data-model-picker-trigger]");
  await expect(model).toContainText("An exceptionally long model name");
  const project = page.locator(".session-composer [data-cwd-picker-trigger]");
  expect(
    await project
      .locator("span")
      .first()
      .evaluate((el) => getComputedStyle(el).fontSize),
  ).toBe("13px");
  expect(
    await page
      .locator("[data-git-picker-trigger] span")
      .first()
      .evaluate((el) => getComputedStyle(el).fontSize),
  ).toBe("13px");
  await model.click();
  await expect(page.locator("[data-model-picker]")).toBeVisible();
  await page.keyboard.press("Escape");
  const access = page.locator(".session-composer [data-access-picker-trigger]");
  expect(
    await model
      .locator("span")
      .first()
      .evaluate((el) => getComputedStyle(el).fontSize),
  ).toBe("12px");
  expect(
    await project
      .locator("svg")
      .first()
      .evaluate((el) => el.getBoundingClientRect().width),
  ).toBe(16);
  expect(
    await access
      .locator("svg")
      .first()
      .evaluate((el) => el.getBoundingClientRect().width),
  ).toBe(16);
  const modelBounds = await box(model);
  const accessBounds = await box(access);
  expect(accessBounds.x - modelBounds.x - modelBounds.width).toBe(8);
  await access.scrollIntoViewIfNeeded();
  await expect(access).toBeInViewport({ ratio: 1 });
  await access.click();
  await expect(page.getByRole("listbox")).toBeVisible();
  await page.keyboard.press("Escape");
  await project.click();
  await expect(page.locator("[data-cwd-picker]")).toBeVisible();
  await page.keyboard.press("Escape");
  await page.locator("[data-git-picker-trigger]").click();
  await expect(page.locator("[data-branch-picker]")).toBeVisible();
  await page.keyboard.press("Escape");
  for (const button of [
    model,
    access,
    page.getByRole("button", { name: "Add files or choose a mode" }),
    page.getByRole("button", { name: "Send", exact: true }),
  ]) {
    expect((await box(button)).height).toBe(30);
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBe(
    320,
  );
});

test("edit-mode Cancel stays aligned and usable in a narrow composer", async ({
  page,
}) => {
  await page.setViewportSize({ width: 320, height: 720 });
  await page.goto("/tests/browser/ui-review.html?closed&edit");
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          typeof (window as Window & { reviewBeginEdit?: () => void })
            .reviewBeginEdit,
      ),
    )
    .toBe("function");
  await page.evaluate(() =>
    (window as Window & { reviewBeginEdit: () => void }).reviewBeginEdit(),
  );
  const cancel = page.getByRole("button", {
    name: "Stop editing last message",
  });
  await expect(cancel).toBeVisible();
  expect((await box(cancel)).height).toBe(30);
  await expect(cancel).toBeInViewport({ ratio: 1 });
  await cancel.click();
  await expect(cancel).toHaveCount(0);
});

test("opening and closing motion preserves adjacency and active-tab visibility", async ({
  page,
}) => {
  await page.setViewportSize({ width: 320, height: 720 });
  await page.goto("/tests/browser/ui-review.html?closed&overflow");
  await page.evaluate(() =>
    localStorage.setItem("monocode.tabAnimationsEnabled", "1"),
  );
  await page.reload();
  await sessionPlus(page).click();
  await terminalPlus(page).click();
  await expect(page.locator("[data-opening-tab]")).toHaveCount(0);
  for (const [strip, active] of [
    [
      page.locator("[data-title-tab-strip]"),
      page.locator('[data-title-tab-id="new-1"]'),
    ],
    [
      page.getByRole("tablist", { name: "Terminals" }),
      page
        .getByRole("tablist", { name: "Terminals" })
        .getByRole("tab", { selected: true }),
    ],
  ]) {
    await expect
      .poll(async () => {
        const s = await box(strip);
        const t = await box(active);
        return t.x >= s.x - 1 && t.x + t.width <= s.x + s.width + 1;
      })
      .toBe(true);
  }
  await expect(
    page.getByRole("button", { name: "Scroll tabs left" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Scroll tabs left" }).click();
  await expect(
    page.getByRole("button", { name: "Scroll tabs right" }),
  ).toBeVisible();
  await expect(sessionPlus(page)).toBeInViewport({ ratio: 1 });
  await page.setViewportSize({ width: 1120, height: 720 });
  await page
    .getByRole("button", { name: "Close Terminal 1", exact: true })
    .click();
  await expect(page.locator("[data-closing-tab]")).toHaveCount(0);
  const terminals = page.getByRole("tablist", { name: "Terminals" });
  await adjacent(
    terminals.locator("[data-tab-slot-id]").last(),
    terminalPlus(page),
  );
});

test("terminal reordering still works beside the adjacent action", async ({
  page,
}) => {
  await page.goto("/tests/browser/ui-review.html?closed");
  await terminalPlus(page).click();
  const tabs = page.getByRole("tablist", { name: "Terminals" });
  const first = await box(
    tabs.getByRole("tab", { name: "Terminal 1", exact: true }),
  );
  const second = await box(
    tabs.getByRole("tab", { name: "Terminal 2", exact: true }),
  );
  await page.mouse.move(first.x + first.width / 2, first.y + first.height / 2);
  await page.mouse.down();
  await page.mouse.move(
    second.x + second.width / 2,
    second.y + second.height / 2,
    { steps: 20 },
  );
  await page.mouse.up();
  await expect(tabs.getByRole("tab")).toHaveText(["Terminal 2", "Terminal 1"]);
  await expect(page.locator("[data-terminal-created]")).toHaveText("1");
  await adjacent(tabs.locator("[data-tab-slot-id]").last(), terminalPlus(page));
});

test("multiline mentions, scrolling, selection and IME retain matching overlay geometry", async ({
  page,
}) => {
  await page.setViewportSize({ width: 320, height: 720 });
  await page.goto("/tests/browser/ui-review.html?closed");
  await field(page).fill("@App.tsx ");
  await expect(
    page.locator(".composer-highlight .text-mention").first(),
  ).toContainText("App.tsx");
  const value =
    "@App.tsx " +
    "a long line that wraps in a narrow pane ".repeat(30) +
    "\n".repeat(3);
  await field(page).fill(value);
  const metrics = await field(page).evaluate((el) => {
    const overlay = document.querySelector(".composer-highlight")!;
    const keys = [
      "fontSize",
      "lineHeight",
      "fontFamily",
      "fontWeight",
      "letterSpacing",
      "paddingLeft",
      "paddingRight",
      "paddingTop",
      "paddingBottom",
      "textIndent",
      "whiteSpace",
      "overflowWrap",
    ] as const;
    const f = getComputedStyle(el);
    const h = getComputedStyle(overlay);
    return {
      same: keys.every((key) => f[key] === h[key]),
      font: f.fontSize,
      line: f.lineHeight,
      width: el.clientWidth === overlay.clientWidth,
    };
  });
  expect(metrics).toEqual({
    same: true,
    font: "15px",
    line: "24px",
    width: true,
  });
  await field(page).evaluate((el: HTMLTextAreaElement) => {
    el.scrollTop = el.scrollHeight;
    el.dispatchEvent(new Event("scroll", { bubbles: true }));
    el.setSelectionRange(1, 8);
  });
  expect(
    await field(page).evaluate((el: HTMLTextAreaElement) => [
      el.selectionStart,
      el.selectionEnd,
    ]),
  ).toEqual([1, 8]);
  expect(await field(page).evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
  expect(await field(page).evaluate((el) => el.scrollTop)).toBe(
    await page.locator(".composer-highlight").evaluate((el) => el.scrollTop),
  );
  await field(page).dispatchEvent("compositionstart", { data: "日" });
  await field(page).dispatchEvent("keydown", {
    key: "Enter",
    isComposing: true,
  });
  await expect(page.locator("[data-sent]")).toBeEmpty();
  await field(page).dispatchEvent("compositionend", { data: "日本語" });
  await page
    .getByRole("button", { name: "Add files or choose a mode" })
    .click();
  await page.getByRole("button", { name: "Upload file" }).click();
  await expect(page.locator(".session-composer")).toContainText("notes.txt");
  await expect(field(page)).toBeFocused();
});

for (const light of [false, true]) {
  for (const wallpaper of [false, true]) {
    test(`composer contrast in ${light ? "light" : "dark"} theme, wallpaper ${wallpaper}`, async ({
      page,
    }, testInfo) => {
      await page.goto(
        `/tests/browser/ui-review.html?closed&${light ? "light&" : ""}${wallpaper ? "wallpaper" : ""}`,
      );
      await expect(field(page)).toBeVisible();
      const contrasts = await page.evaluate((wallpaper) => {
        const canvas = document.createElement("canvas");
        canvas.width = canvas.height = 1;
        const ctx = canvas.getContext("2d")!;
        const field = document.querySelector(".session-composer textarea")!;
        const surface = getComputedStyle(
          document.querySelector("[data-composer-box]")!,
        );
        const text = getComputedStyle(field, "::placeholder");
        const content = getComputedStyle(
          document.documentElement,
        ).getPropertyValue("--color-content");
        const tint = surface.backgroundImage.match(/color\(srgb [^)]+\)/)?.[0];
        if (!tint) throw new Error("Missing composer tint");
        const luminance = (rgb: number[]) =>
          rgb
            .slice(0, 3)
            .map((c) => c / 255)
            .map((c) =>
              c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4,
            )
            .reduce((l, c, i) => l + c * [0.2126, 0.7152, 0.0722][i], 0);
        return (
          wallpaper
            ? ["#355e83", "#978574", "#486b59"]
            : [getComputedStyle(document.body).backgroundColor]
        ).map((backdrop) => {
          ctx.clearRect(0, 0, 1, 1);
          ctx.fillStyle = getComputedStyle(
            document.documentElement,
          ).getPropertyValue("--color-background-base");
          ctx.fillRect(0, 0, 1, 1);
          ctx.fillStyle = backdrop;
          ctx.fillRect(0, 0, 1, 1);
          ctx.fillStyle = surface.backgroundColor;
          ctx.fillRect(0, 0, 1, 1);
          ctx.fillStyle = tint;
          ctx.fillRect(0, 0, 1, 1);
          const bg = luminance([...ctx.getImageData(0, 0, 1, 1).data]);
          const ratios = [text.color, content].map((color) => {
            const saved = ctx.getImageData(0, 0, 1, 1);
            ctx.fillStyle = color;
            ctx.fillRect(0, 0, 1, 1);
            const fg = luminance([...ctx.getImageData(0, 0, 1, 1).data]);
            ctx.putImageData(saved, 0, 0);
            return (Math.max(fg, bg) + 0.05) / (Math.min(fg, bg) + 0.05);
          });
          return {
            backdrop,
            placeholder: ratios[0],
            input: ratios[1],
            placeholderFill: text.webkitTextFillColor === text.color,
          };
        });
      }, wallpaper);
      await testInfo.attach("contrast", {
        body: JSON.stringify(contrasts, null, 2),
        contentType: "application/json",
      });
      for (const contrast of contrasts) {
        expect(contrast.placeholder).toBeGreaterThanOrEqual(4.5);
        expect(contrast.input).toBeGreaterThanOrEqual(4.5);
        expect(contrast.placeholderFill).toBe(true);
      }
    });
  }
}

for (const width of [320, 1120]) {
  test(`composer remains slightly darker as main glass opacity changes at ${width}px`, async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.setViewportSize({ width, height: 800 });
    await page.goto(`/tests/browser/ui-review.html?glass&wallpaper${width === 320 ? "&closed" : ""}`);
    await expect(field(page)).toBeVisible();
    const samples: number[] = [];
    for (const opacity of [0.9, 0.5]) {
      await page.evaluate(async (opacity) => {
        const modulePath = "/src/features/settings/model/appearance.ts";
        const appearance = await import(modulePath);
        appearance.applySidebarOpacity(opacity);
      }, opacity);
      const colors = await page.evaluate(async () => {
        // Composite the actual pane/composer paints over a sample desktop.
        await document.fonts.ready;
        const pane = getComputedStyle(document.querySelector("main")!);
        const surface = getComputedStyle(document.querySelector("[data-composer-box]")!);
        const canvas = document.createElement("canvas");
        canvas.width = canvas.height = 1;
        const ctx = canvas.getContext("2d")!;
        ctx.fillStyle = "#808080"; ctx.fillRect(0, 0, 1, 1);
        ctx.fillStyle = pane.backgroundColor; ctx.fillRect(0, 0, 1, 1);
        const paneColor = ctx.getImageData(0, 0, 1, 1).data[0];
        ctx.fillStyle = surface.backgroundColor; ctx.fillRect(0, 0, 1, 1);
        const tint = surface.backgroundImage.match(/color\(srgb [^)]+\)/)?.[0];
        if (tint) { ctx.fillStyle = tint; ctx.fillRect(0, 0, 1, 1); }
        return { pane: paneColor, composer: ctx.getImageData(0, 0, 1, 1).data[0] };
      });
      expect(colors.composer).toBeLessThan(colors.pane);
      expect(colors.composer).toBeGreaterThanOrEqual(colors.pane * 0.75);
      samples.push(colors.composer);
    }
    expect(samples[1]).toBeGreaterThan(samples[0]);

    await page.evaluate(() => document.documentElement.classList.remove("glass-body"));
    const solid = await page.locator("[data-composer-box]").evaluate((el) => getComputedStyle(el).backgroundColor);
    await page.evaluate(() => {
      document.documentElement.classList.add("glass-body");
      document.documentElement.classList.remove("has-native-glass");
    });
    await expect(page.locator("[data-composer-box]")).toHaveCSS("background-color", solid);
  });
}
