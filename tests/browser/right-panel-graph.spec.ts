import { expect, test } from "@playwright/test";

test.use({ viewport: { width: 1400, height: 900 } });

test("gives History a persistent tab and working toolbar while classic keeps its graph", async ({
  page,
}, testInfo) => {
  test.setTimeout(60_000);
  await page.addInitScript(() => {
    localStorage.setItem("monocode.insetLayout", "1");
    localStorage.setItem("monocode.rightPanelOpen", "1");
    if (localStorage.getItem("monocode.rightPanelTab") == null)
      localStorage.setItem("monocode.rightPanelTab", "changes");
    const sha = (index: number) => index.toString(16).padStart(40, "0");
    const commits = Array.from({ length: 80 }, (_, index) => ({
      sha: sha(index + 1),
      shortSha: sha(index + 1).slice(-7),
      parents: index < 79 ? [sha(index + 2)] : [],
      subject: `Graph commit ${index + 1}`,
      author: "Fixture author",
      timestamp: 1_760_000_000 - index * 60,
      refs: index === 0 ? [{ name: "main", kind: "local" }] : [],
      head: index === 0,
    }));
    // Several merged branches make all lane and label colors visible in previews.
    const parents = [
      [2, 6],
      [3, 8],
      [4, 10],
      [5, 12],
      [14, 16],
      [7],
      [14],
      [9],
      [14],
      [11],
      [14],
      [13],
      [14],
      [15],
      [17],
      [17],
    ];
    parents.forEach((ids, index) => {
      commits[index]!.parents = ids.map(sha);
    });
    for (const [index, name, kind] of [
      [5, "origin/main", "remote"],
      [7, "feature/sidebar", "local"],
      [9, "fix/streaming", "local"],
      [11, "feature/search", "local"],
      [15, "feature/history", "local"],
    ] as const)
      commits[index]!.refs = [{ name, kind }];
    const historyWindow = window as typeof window & {
      historyCommands: string[];
    };
    historyWindow.historyCommands = [];
    let internals: Record<string, unknown>;
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      get: () => internals,
      set: (value) => {
        const original = value.invoke;
        value.invoke = (command: string, args: unknown) => {
          if (command === "git_history") {
            historyWindow.historyCommands.push(command);
            return Promise.resolve({ head: sha(1), commits });
          }
          if (command === "git_commit_files") return Promise.resolve([]);
          return original(command, args);
        };
        internals = value;
      },
    });
  });
  await page.goto("/tests/browser/workspace-performance.html");
  const panel = page.locator('[aria-label="Project tools"]');
  await expect(panel).toBeVisible();
  const graphTab = panel.getByRole("tab", { name: "History", exact: true });
  const explorer = panel.getByRole("tab", { name: "Explorer", exact: true });
  const changes = panel.getByRole("tab", { name: /^Changes/ });
  await expect(changes).toHaveAttribute("aria-selected", "true");
  const checkHeader = async (name: string) => {
    const header = panel.getByRole("toolbar", { name, exact: true });
    await expect(header).toBeVisible();
    const bounds = (await header.boundingBox())!;
    expect(bounds.height).toBe(32);
    for (const button of await header.getByRole("button").all()) {
      const action = (await button.boundingBox())!;
      expect(action.height).toBe(24);
      expect(action.y + action.height / 2).toBeCloseTo(
        bounds.y + bounds.height / 2 - 0.5,
      );
      expect(action.x).toBeGreaterThanOrEqual(bounds.x);
      expect(action.x + action.width).toBeLessThanOrEqual(
        bounds.x + bounds.width,
      );
    }
    return header;
  };
  const changesHeader = await checkHeader("Changes");
  const headerTop = (await changesHeader.boundingBox())!.y;
  await changesHeader.screenshot({
    path: testInfo.outputPath("changes-header.png"),
  });
  await explorer.click();
  const explorerHeader = await checkHeader("Explorer");
  expect((await explorerHeader.boundingBox())!.y).toBe(headerTop);
  await explorerHeader.screenshot({
    path: testInfo.outputPath("explorer-header.png"),
  });
  await expect(
    panel.getByRole("button", { name: /^(Expand|Collapse) graph$/ }),
  ).toHaveCount(0);

  await graphTab.click();
  const historyHeader = await checkHeader("History");
  expect((await historyHeader.boundingBox())!.y).toBe(headerTop);
  const graph = panel.getByRole("tabpanel", { name: "History", exact: true });
  await expect(graph).toBeVisible();
  await expect(graph.locator(".git-history-item")).toHaveCount(80);
  await expect(
    panel.getByRole("button", { name: /^(Expand|Collapse) graph$/ }),
  ).toHaveCount(0);
  const panelBounds = await panel.boundingBox();
  const graphBounds = await graph.boundingBox();
  expect(graphBounds!.height).toBeGreaterThan(panelBounds!.height - 45);
  await graph.locator(".git-history-item").first().click();
  await expect(graph.locator(".git-history-item").first()).toHaveAttribute(
    "aria-pressed",
    "true",
  );

  const toolbar = graph.getByRole("toolbar", { name: "History", exact: true });
  await expect(toolbar.getByText("main", { exact: true })).toBeVisible();
  await expect(toolbar.getByRole("button")).toHaveCount(2);
  await expect(
    toolbar.getByRole("button", { name: "Refresh history" }),
  ).toBeVisible();
  await expect(toolbar).not.toContainText(/commits/i);
  await expect(graph.getByRole("textbox")).toHaveCount(0);
  await expect(graph.locator("time").first()).toBeVisible();

  await toolbar
    .getByRole("button", { name: "Search history", exact: true })
    .click();
  const search = toolbar.getByRole("textbox", { name: "Search commits" });
  await expect(search).toBeFocused();
  await checkHeader("History");
  await expect(toolbar.getByRole("button")).toHaveCount(1);
  await expect(
    toolbar.getByRole("button", { name: "Refresh history" }),
  ).toHaveCount(0);
  await expect(toolbar.getByText("main", { exact: true })).toHaveCount(0);
  await search.fill("COMMIT 80");
  await expect(graph.locator(".git-history-item")).toHaveCount(1);
  await expect(graph.locator(".git-history-graph path")).toHaveCount(0);
  await search.fill("0000050");
  await expect(graph.locator(".git-history-item")).toHaveCount(1);
  await search.fill("feature/sidebar");
  await expect(graph.locator(".git-history-item")).toHaveCount(1);
  await search.fill("Fixture author");
  await expect(graph.locator(".git-history-item")).toHaveCount(80);
  await search.fill("nothing matches");
  await expect(graph.getByText("No matching commits")).toBeVisible();
  await search.press("Escape");
  await expect(search).toHaveCount(0);
  await expect(
    toolbar.getByRole("button", { name: "Search history", exact: true }),
  ).toBeFocused();
  await expect(graph.locator(".git-history-item")).toHaveCount(80);
  expect(
    await graph.locator(".git-history-graph path").count(),
  ).toBeGreaterThan(0);
  await toolbar
    .getByRole("button", { name: "Search history", exact: true })
    .click();
  await search.fill("commit 80");
  await toolbar.getByRole("button", { name: "Close history search" }).click();
  await expect(search).toHaveCount(0);
  await expect(graph.locator(".git-history-item")).toHaveCount(80);

  const commands = () =>
    page.evaluate(
      () =>
        (window as typeof window & { historyCommands: string[] })
          .historyCommands,
    );
  const beforeRefresh = (await commands()).filter(
    (command) => command === "git_history",
  ).length;
  await graph.getByRole("button", { name: "Refresh history" }).click();
  await expect
    .poll(
      async () =>
        (await commands()).filter((command) => command === "git_history")
          .length,
    )
    .toBeGreaterThan(beforeRefresh);
  const from = await graphTab.boundingBox();
  const to = await explorer.boundingBox();
  await page.mouse.move(from!.x + from!.width / 2, from!.y + from!.height / 2);
  await page.mouse.down();
  await page.mouse.move(to!.x + 4, to!.y + to!.height / 2, { steps: 12 });
  await page.mouse.up();
  await expect(panel.getByRole("tab").first()).toHaveText("History");
  await page.reload();
  await expect(graphTab).toHaveAttribute("aria-selected", "true");
  await expect(panel.getByRole("tab").first()).toHaveText("History");
  await expect(graph.locator(".git-history-item")).toHaveCount(80);
  await graph.screenshot({ path: testInfo.outputPath("history-tab.png") });

  const resize = async (width: number) => {
    const sash = await page
      .getByRole("separator", { name: "Resize right sidebar", exact: true })
      .boundingBox();
    const bounds = await panel.boundingBox();
    await page.mouse.move(sash!.x + 2, sash!.y + sash!.height / 2);
    await page.mouse.down();
    await page.mouse.move(
      sash!.x + 2 + bounds!.width - width,
      sash!.y + sash!.height / 2,
      { steps: 12 },
    );
    await page.mouse.up();
    await expect
      .poll(async () => Math.abs((await panel.boundingBox())!.width - width))
      .toBeLessThanOrEqual(1);
    const panelBounds = (await panel.boundingBox())!;
    for (const name of ["Search history", "Refresh history"]) {
      const button = await graph
        .getByRole("button", { name, exact: true })
        .boundingBox();
      expect(button!.x).toBeGreaterThanOrEqual(panelBounds.x);
      expect(button!.x + button!.width).toBeLessThanOrEqual(
        panelBounds.x + panelBounds.width,
      );
    }
  };
  await resize(220);
  await changes.click();
  await checkHeader("Changes");
  await explorer.click();
  await checkHeader("Explorer");
  await graphTab.click();
  await checkHeader("History");
  await graph.screenshot({ path: testInfo.outputPath("history-narrow.png") });
  await toolbar
    .getByRole("button", { name: "Search history", exact: true })
    .click();
  await expect(search).toBeFocused();
  await search.fill("feature/");
  await checkHeader("History");
  await graph.screenshot({
    path: testInfo.outputPath("history-search-narrow.png"),
  });
  await toolbar.getByRole("button", { name: "Close history search" }).click();
  await resize(640);
  await page.evaluate(async () => {
    const path = "/src/features/settings/model/appearance.ts";
    const appearance = await import(path);
    appearance.applyThemePreference("light");
  });
  await graph.screenshot({
    path: testInfo.outputPath("history-wide-light.png"),
  });
  await toolbar
    .getByRole("button", { name: "Search history", exact: true })
    .click();
  await search.fill("feature/");
  await checkHeader("History");
  await graph.screenshot({
    path: testInfo.outputPath("history-search-light.png"),
  });
  await toolbar.getByRole("button", { name: "Close history search" }).click();

  await changes.click();
  await expect(graph).toHaveCount(0);
  await expect(
    panel.getByRole("button", { name: /^(Expand|Collapse) graph$/ }),
  ).toHaveCount(0);

  await page.evaluate(async () => {
    const path = "/src/features/settings/model/appearance.ts";
    const appearance = await import(path);
    appearance.saveInsetLayout(false);
  });
  await expect(page.locator(".inset-shell")).toHaveCount(0);
  await page.getByRole("tab", { name: /^Changes/ }).click();
  expect(
    (await page
      .getByRole("toolbar", { name: "Changes", exact: true })
      .boundingBox())!.height,
  ).toBe(36);
  await expect(
    page.getByRole("button", { name: "Collapse graph", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("tab", { name: "History", exact: true }),
  ).toHaveCount(0);
});
