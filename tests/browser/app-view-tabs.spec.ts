import { expect, test } from "@playwright/test";

test.use({ viewport: { width: 1300, height: 900 } });

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem("monocode.insetLayout", "1");
  });
});

test("app tabs retain their state and keep usage and resource pressure in the footer", async ({
  page,
}) => {
  test.setTimeout(60_000);
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
        ].includes(command)
      ) {
        return Promise.resolve([]);
      }
      if (command === "resource_usage")
        return Promise.resolve({ cpuPercent: 95, memoryPressure: 2 });
      return base(command, args);
    };
  });

  const nav = page.getByRole("navigation", { name: "Views" });
  const appTab = (view: string) =>
    page.locator(`[data-title-tab-id="app-view:${view}"] [data-title-tab]`);
  const footer = page.getByRole("contentinfo", { name: "Provider usage" });

  for (const view of ["inbox", "search", "notes", "automations"]) {
    await nav
      .getByRole("button", { name: new RegExp(`^${view}`, "i") })
      .click();
    await expect(appTab(view)).toHaveAttribute("data-title-tab-active", "true");
    await expect(page.locator(`[data-app-${view}]`)).toBeVisible();
    await expect(
      page.locator(`[data-app-${view}] [data-app-view-header]`),
    ).toHaveCount(0);
    await expect(footer).toBeVisible();
  }

  await page
    .getByPlaceholder("Filter automations")
    .fill("saved automation filter");
  await appTab("notes").click();
  await page.getByPlaceholder("Filter notes").fill("saved notes filter");
  await appTab("search").click();
  await page
    .getByRole("textbox", { name: "Search", exact: true })
    .fill("saved search query");
  await appTab("automations").click();
  await expect(page.getByPlaceholder("Filter automations")).toHaveValue(
    "saved automation filter",
  );
  await appTab("notes").click();
  await expect(page.getByPlaceholder("Filter notes")).toHaveValue(
    "saved notes filter",
  );
  await appTab("search").click();
  await expect(
    page.getByRole("textbox", { name: "Search", exact: true }),
  ).toHaveValue("saved search query");
  await nav.getByRole("button", { name: /^search/i }).click();
  await expect(appTab("search")).toHaveCount(1);

  await expect(
    footer.getByRole("button", { name: "High resource usage", exact: true }),
  ).toBeVisible({ timeout: 15_000 });
  for (const view of ["inbox", "notes", "automations", "search"]) {
    await appTab(view).click();
    await expect(
      footer.getByRole("button", { name: "High resource usage", exact: true }),
    ).toBeVisible();
    const bounds = await footer.boundingBox();
    expect(bounds!.y + bounds!.height).toBeGreaterThanOrEqual(898);
  }

  const modifier = await page.evaluate(() =>
    /Mac|iPhone|iPad/.test(navigator.platform) ? "Meta" : "Control",
  );
  await page.keyboard.press(`${modifier}+1`);
  await expect(page.locator("[data-app-search]")).toHaveCount(0);
  await appTab("search").click();
  await expect(appTab("search")).toHaveAttribute(
    "data-title-tab-active",
    "true",
  );
  await page.keyboard.press(`${modifier}+w`);
  await expect(
    page.locator('[data-title-tab-id="app-view:search"]'),
  ).toHaveCount(0);
  await expect(appTab("notes")).toHaveAttribute(
    "data-title-tab-active",
    "true",
  );
  await expect(page.locator('[data-title-tab-active="true"]')).toHaveCount(1);
  await expect(footer).toBeVisible();
});

test("closing a tab selects its neighbor across app and session tabs", async ({
  page,
}) => {
  test.setTimeout(60_000);
  // Linux also shows a window title beside the strip. Keep all eight tabs
  // visible so dragging Notes starts on the tab, not outside the scroll clip.
  await page.setViewportSize({ width: 2400, height: 900 });
  await page.goto("/tests/browser/workspace-performance.html");
  await page.locator(".inset-shell").waitFor();
  const nav = page.getByRole("navigation", { name: "Views" });
  const tab = (id: string) =>
    page.locator(`[data-title-tab-id="${id}"] [data-title-tab]`);
  for (const view of ["inbox", "search", "notes", "automations"]) {
    await nav
      .getByRole("button", { name: new RegExp(`^${view}`, "i") })
      .click();
    await expect(tab(`app-view:${view}`)).toHaveAttribute(
      "data-title-tab-active",
      "true",
    );
  }
  const modifier = await page.evaluate(() =>
    /Mac|iPhone|iPad/.test(navigator.platform) ? "Meta" : "Control",
  );
  const closeWithShortcut = async () => {
    // Let the tab-close animation settle between repeated shortcut presses.
    await page.waitForTimeout(180);
    await page.keyboard.press(`${modifier}+w`);
  };
  await tab("app-view:notes").click();
  await closeWithShortcut();
  await expect(tab("app-view:automations")).toHaveAttribute(
    "data-title-tab-active",
    "true",
  );
  await expect(tab("app-view:notes")).toHaveCount(0);
  await closeWithShortcut();
  await expect(tab("app-view:search")).toHaveAttribute(
    "data-title-tab-active",
    "true",
  );

  // Closing an inactive app tab must leave Search selected.
  await page
    .locator('[data-title-tab-id="app-view:inbox"]')
    .getByRole("button", { name: "Close Inbox", exact: true })
    .click();
  await expect(tab("app-view:search")).toHaveAttribute(
    "data-title-tab-active",
    "true",
  );

  const workspaceTabs = await page
    .locator('[data-title-tab-id]:not([data-title-tab-id^="app-view:"])')
    .evaluateAll((tabs) =>
      tabs.map((tab) => tab.getAttribute("data-title-tab-id")!),
    );

  // Reordering an app tab between sessions changes its close destination.
  await nav.getByRole("button", { name: "Notes", exact: true }).click();
  await expect(tab("app-view:notes")).toHaveAttribute(
    "data-title-tab-active",
    "true",
  );
  await expect(
    page.locator("[data-opening-tab], [data-closing-tab]"),
  ).toHaveCount(0);
  await expect(tab("app-view:notes")).toBeInViewport();
  await expect(tab(workspaceTabs[1])).toBeInViewport();
  const from = await tab("app-view:notes").boundingBox();
  const to = await tab(workspaceTabs[1]).boundingBox();
  await page.mouse.move(from!.x + from!.width / 2, from!.y + from!.height / 2);
  await page.mouse.down();
  await page.mouse.move(to!.x + to!.width / 2, to!.y + to!.height / 2, {
    steps: 12,
  });
  await page.mouse.up();
  await page.waitForTimeout(400);
  const reordered = await page
    .locator("[data-title-tab-id]")
    .evaluateAll((tabs) =>
      tabs.map((tab) => tab.getAttribute("data-title-tab-id")!),
    );
  const noteIndex = reordered.indexOf("app-view:notes");
  const nextToNotes = reordered[noteIndex + 1];
  expect(noteIndex).toBeLessThan(reordered.indexOf("app-view:search"));
  expect(nextToNotes).not.toMatch(/^app-view:/);
  await tab("app-view:notes").click();
  await closeWithShortcut();
  await expect(tab(nextToNotes)).toHaveAttribute(
    "data-title-tab-active",
    "true",
  );

  const lastWorkspaceId = workspaceTabs.at(-1)!;
  await tab(lastWorkspaceId).click();
  await closeWithShortcut();
  await expect(tab(lastWorkspaceId)).toHaveCount(0);
  await expect(tab("app-view:search")).toHaveAttribute(
    "data-title-tab-active",
    "true",
  );

  // Search is now at the right edge, so closing it selects the last session.
  await closeWithShortcut();
  await expect(tab("app-view:search")).toHaveCount(0);
  await expect(tab(workspaceTabs.at(-2)!)).toHaveAttribute(
    "data-title-tab-active",
    "true",
  );
});

test("sidebar session selection switches away from app tabs", async ({
  page,
}) => {
  test.setTimeout(60_000);
  await page.goto("/tests/browser/workspace-performance.html");
  await page.locator(".inset-shell").waitFor();
  await page.evaluate(() => {
    const fixture = window as unknown as {
      workspaceSessions: Record<string, unknown>[];
      __TAURI_INTERNALS__: {
        invoke: (
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<unknown>;
      };
    };
    // Resumed sessions are already saved, so closing them need not write again.
    const records = new Map(
      fixture.workspaceSessions.map((session) => [
        String(session.id),
        { ...session, createdAt: Date.now(), updatedAt: Date.now() },
      ]),
    );
    const base = fixture.__TAURI_INTERNALS__.invoke;
    fixture.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "session_upsert") {
        const session = args!.session as Record<string, unknown>;
        const record = {
          ...session,
          createdAt: Date.now(),
          updatedAt: Date.now(),
        };
        records.set(String(session.id), record);
        return Promise.resolve(record);
      }
      if (command === "session_list_by_project")
        return Promise.resolve(
          [...records.values()].filter((record) => record.cwd === args?.cwd),
        );
      if (command === "session_get")
        return Promise.resolve(records.get(String(args?.sessionId)) ?? null);
      return base(command, args);
    };
  });
  const nav = page.getByRole("navigation", { name: "Views" });
  const appTab = (view: string) =>
    page.locator(`[data-title-tab-id="app-view:${view}"] [data-title-tab]`);
  const sessionCard = page.locator(
    '[data-session-select="workspace-session-1"]',
  );
  const sessionPane = page.locator('[data-session-drop="workspace-session-1"]');

  // Both selecting an existing tab and reopening a closed tab must reveal it.
  for (const view of ["inbox", "search", "notes", "automations"]) {
    await nav
      .getByRole("button", { name: new RegExp(`^${view}`, "i") })
      .click();
    await expect(page.locator(`[data-app-${view}]`)).toBeVisible();
    await sessionCard.click();
    await expect(sessionPane).toBeVisible();
    await expect(page.locator(`[data-app-${view}]`)).toBeHidden();
    await expect(appTab(view)).toHaveCount(1);
    await expect(appTab(view)).not.toHaveAttribute(
      "data-title-tab-active",
      "true",
    );
    await expect(page.locator('[data-title-tab-active="true"]')).toContainText(
      "Session 2",
    );

    await page
      .locator('[data-title-tab-id]:has([data-title-tab-active="true"])')
      .getByRole("button", { name: "Close Session 2", exact: true })
      .click();
    await expect(sessionPane).toHaveCount(0);
    await appTab(view).click();
    await expect(page.locator(`[data-app-${view}]`)).toBeVisible();
    await sessionCard.click();
    await expect(sessionPane).toBeVisible();
    await expect(page.locator(`[data-app-${view}]`)).toBeHidden();
    await expect(appTab(view)).toHaveCount(1);
    await expect(appTab(view)).not.toHaveAttribute(
      "data-title-tab-active",
      "true",
    );
    await expect(page.locator('[data-title-tab-active="true"]')).toContainText(
      "Session 2",
    );
  }
});

test("new tabs append chronologically across sessions and app views", async ({
  page,
}) => {
  await page.goto("/tests/browser/workspace-performance.html");
  await page.locator(".inset-shell").waitFor();
  const nav = page.getByRole("navigation", { name: "Views" });
  const tab = (id: string) =>
    page.locator(`[data-title-tab-id="${id}"] [data-title-tab]`);
  const tabIds = () =>
    page
      .locator("[data-title-tab-id]")
      .evaluateAll((tabs) =>
        tabs.map((tab) => tab.getAttribute("data-title-tab-id")!),
      );
  const initial = await tabIds();
  const modifier = await page.evaluate(() =>
    /Mac|iPhone|iPad/.test(navigator.platform) ? "Meta" : "Control",
  );
  for (const view of ["inbox", "notes"]) {
    await nav
      .getByRole("button", { name: new RegExp(`^${view}`, "i") })
      .click();
    await expect(tab(`app-view:${view}`)).toHaveAttribute(
      "data-title-tab-active",
      "true",
    );
  }
  await page.keyboard.press(`${modifier}+t`);
  await expect(page.locator("[data-title-tab-id]")).toHaveCount(
    initial.length + 3,
  );
  const firstNew = (await tabIds()).find(
    (id) => !initial.includes(id) && !id.startsWith("app-view:"),
  )!;
  const expected = [...initial, "app-view:inbox", "app-view:notes", firstNew];
  await expect.poll(tabIds).toEqual(expected);

  await nav.getByRole("button", { name: /^search/i }).click();
  await expect(tab("app-view:search")).toHaveAttribute(
    "data-title-tab-active",
    "true",
  );
  expected.push("app-view:search");
  await expect.poll(tabIds).toEqual(expected);

  // Opening from an earlier tab still appends instead of inserting beside it.
  await tab(initial[0]).click();
  await expect(tab(initial[0])).toHaveAttribute(
    "data-title-tab-active",
    "true",
  );
  await page.keyboard.press(`${modifier}+t`);
  await expect(page.locator("[data-title-tab-id]")).toHaveCount(
    expected.length + 1,
  );
  const secondNew = (await tabIds()).find((id) => !expected.includes(id))!;
  expected.push(secondNew);
  await expect.poll(tabIds).toEqual(expected);

  await nav.getByRole("button", { name: /^inbox/i }).click();
  await expect(tab("app-view:inbox")).toHaveAttribute(
    "data-title-tab-active",
    "true",
  );
  await expect.poll(tabIds).toEqual(expected);

  // A closed and reopened app tab is newly opened and belongs at the end.
  await page
    .locator('[data-title-tab-id="app-view:inbox"]')
    .getByRole("button", { name: "Close Inbox", exact: true })
    .click();
  await expect(tab("app-view:inbox")).toHaveCount(0);
  await nav.getByRole("button", { name: /^inbox/i }).click();
  await expect
    .poll(tabIds)
    .toEqual([
      ...expected.filter((id) => id !== "app-view:inbox"),
      "app-view:inbox",
    ]);
});
