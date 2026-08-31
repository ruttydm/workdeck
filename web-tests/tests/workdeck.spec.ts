import { expect, test } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

const surfaces = ["inbox", "workspaces", "git", "search", "pull-requests", "ci", "artifacts", "changes"] as const;
const viewports = {
  minimum: { width: 900, height: 600 },
  intermediate: { width: 1100, height: 720 },
  wide: { width: 1440, height: 900 },
  ultrawide: { width: 1720, height: 1000 }
} as const;
const themes = ["light", "dark"] as const;

async function openWorkdeck(page, path = "/?fixture=polished&area=inbox") {
  await page.goto(path);
  await expect(page.locator(".status-bar")).toBeVisible();
}

test("pointer and keyboard navigation preserve product context", async ({ page }) => {
  await openWorkdeck(page);
  await page.getByRole("button", { name: "Workspaces" }).click();
  await expect(page.getByRole("heading", { name: "Workspaces" })).toBeVisible();
  await page.keyboard.press("Meta+k");
  await expect(page.getByRole("dialog", { name: "Command palette" })).toBeVisible();
  await page.getByRole("button", { name: /Git: Open commits/ }).click();
  await expect(page.locator(".git-surface")).toBeVisible();
  await page.keyboard.press("Meta+1");
  await expect(page.locator(".inbox-surface")).toBeVisible();
});

test("commit and pull-request changes expose every inspection lens without review ceremony", async ({ page }) => {
  await openWorkdeck(page, "/?fixture=polished&area=changes");
  await page.getByRole("tab", { name: "Source" }).click();
  await expect(page.locator(".source-view")).toBeVisible();
  await page.getByRole("tab", { name: "AST" }).click();
  await expect(page.locator(".ast-view")).toBeVisible();
  await expect(page.locator(".review-surface")).toBeVisible();
  await expect(page.getByRole("button", { name: /Capture checkpoint|Mark reviewed/ })).toHaveCount(0);
});

test("code surfaces use semantic highlighting and editor-grade typography", async ({ page }) => {
  await page.setViewportSize(viewports.wide);
  await openWorkdeck(page, "/?fixture=polished&area=changes");

  for (const token of ["keyword", "type", "function", "parameter", "string"]) {
    await expect(page.locator(`.syntax--${token}`).first()).toBeVisible();
  }

  const typography = await page.locator(".syntax-code").first().evaluate((element) => {
    const style = getComputedStyle(element);
    return {
      family: style.fontFamily,
      size: Number.parseFloat(style.fontSize),
      lineHeight: Number.parseFloat(style.lineHeight),
      ligatures: style.fontVariantLigatures
    };
  });
  expect(typography.family).toContain("Geist Mono");
  expect(typography.size).toBeGreaterThanOrEqual(12);
  expect(typography.lineHeight).toBeGreaterThanOrEqual(20);
  expect(typography.ligatures).toContain("common-ligatures");
  await expect(page.locator(".line-number--old").first()).toHaveCSS("position", "sticky");

  await page.getByRole("tab", { name: "Split" }).click();
  await expect(page.getByRole("table", { name: "Before changes" })).toBeVisible();
  await expect(page.getByRole("table", { name: "After changes" })).toBeVisible();
  await expect(page.locator(".diff-line--split .diff-code-cell").first()).toBeVisible();

  await page.goto("/?gallery=1");
  await expect(page.locator(".markdown-inline-code")).toHaveText("OperationId");
  await expect(page.locator(".markdown-code-block .code-language-badge")).toHaveText("Rust");
  await expect(page.locator(".markdown-code")).toHaveCSS("font-family", /Geist Mono/);
});

test("workspace hierarchy discloses and collapses", async ({ page }) => {
  await openWorkdeck(page, "/?fixture=polished&area=workspaces");
  const project = page.locator(".workspace-row--project").first();
  await expect(project).toHaveAttribute("aria-expanded", "true");
  await project.click();
  await expect(project).toHaveAttribute("aria-expanded", "false");
  await page.getByRole("button", { name: "Expand all" }).click();
  await expect(project).toHaveAttribute("aria-expanded", "true");
});

test("workspace navigator switches project context instead of duplicating the hierarchy", async ({ page }) => {
  await openWorkdeck(page, "/?fixture=polished&area=workspaces");
  await page.getByRole("button", { name: /^Workdeck\b/ }).click();
  await expect(page.locator(".workspace-row--project")).toHaveCount(1);
  await expect(page.locator(".workspace-row--project").getByText("Workdeck", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: /All projects/ }).click();
  await expect(page.locator(".workspace-row--project")).toHaveCount(3);
});

test("command palette closes with Escape and returns to the shell", async ({ page }) => {
  await openWorkdeck(page);
  await page.keyboard.press("Meta+k");
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toBeHidden();
  await expect(page.locator(".workbench")).toBeVisible();
});

test("empty onboarding removes portfolio chrome", async ({ page }) => {
  await openWorkdeck(page, "/?fixture=empty&area=inbox");
  await expect(page.locator(".onboarding")).toBeVisible();
  await expect(page.locator(".navigator")).toHaveCount(0);
  await expect(page.locator(".inspector")).toHaveCount(0);
});

test("offline state is explicit without disabling local commits", async ({ page }) => {
  await openWorkdeck(page, "/?fixture=offline&area=inbox");
  await expect(page.getByText("GitHub offline · local commits remain available")).toBeVisible();
  await expect(page.getByText("Provider offline", { exact: true })).toBeVisible();
  await expect(page.locator(".inbox-surface")).toBeVisible();
});

test("pull requests use a minimal list, diff, and changed-files workspace", async ({ page }) => {
  await openWorkdeck(page, "/?fixture=polished&area=pull-requests");
  await expect(page.locator(".navigator")).toHaveCount(0);
  await expect(page.locator(".inspector")).toHaveCount(0);
  await expect(page.getByLabel("Pull request browser")).toBeVisible();
  await expect(page.locator(".task-changes")).toBeVisible();
  await expect(page.getByRole("complementary", { name: "Changed files" })).toBeVisible();
  await expect(page.getByRole("table", { name: "Unified diff" })).toBeVisible();
  await expect(page.locator(".pr-list-scope")).toContainText("Open");
  await expect(page.locator(".pr-row.is-unread")).toHaveCount(2);
  await page.locator(".pr-row.is-unread").first().click();
  await expect(page.locator(".pr-row.is-unread")).toHaveCount(1);
  await expect(page.getByRole("link", { name: "Open on GitHub" })).toBeInViewport();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
    await page.evaluate(() => document.documentElement.clientWidth)
  );
  const master = await page.getByLabel("Pull request browser").boundingBox();
  const detail = await page.locator(".task-file-diff").boundingBox();
  const tree = await page.getByRole("complementary", { name: "Changed files" }).boundingBox();
  expect(master).not.toBeNull();
  expect(detail).not.toBeNull();
  expect(tree).not.toBeNull();
  expect(master!.x + master!.width).toBeLessThanOrEqual(detail!.x + 1);
  expect(detail!.x + detail!.width).toBeLessThanOrEqual(tree!.x + 1);

  const appDirectory = page.getByRole("treeitem", { name: /app \+/ });
  await expect(appDirectory).toHaveAttribute("aria-expanded", "true");
  await appDirectory.click();
  await expect(page.getByRole("treeitem", { name: /OpportunityFeed\.php/ })).toHaveCount(0);
  await appDirectory.click();
  await expect(page.getByRole("treeitem", { name: /OpportunityFeed\.php/ })).toBeVisible();
});

test("one Git rail destination owns fast Commits and Pull requests tabs", async ({ page }) => {
  await openWorkdeck(page, "/?fixture=polished&area=git");
  await expect(page.getByRole("button", { name: "Git", exact: true })).toHaveCount(1);
  await expect(page.locator(".app-rail").getByRole("button", { name: "Pull requests" })).toHaveCount(0);

  const commits = page.getByRole("tab", { name: "Commits", exact: true });
  const pulls = page.getByRole("tab", { name: /Pull requests/ });
  await expect(commits).toHaveAttribute("aria-selected", "true");
  await pulls.click();
  await expect(pulls).toHaveAttribute("aria-selected", "true");
  await expect(page.getByLabel("Pull request browser")).toBeVisible();
  await commits.click();
  await expect(commits).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("region", { name: "Git commit history" })).toBeVisible();
});

test("revisiting Git and pull requests reuses warm content without loading flashes", async ({ page }) => {
  await openWorkdeck(page, "/?fixture=polished&area=git");
  await expect(page.locator(".git-row.is-selected")).toBeVisible();
  await expect(page.locator(".task-changes")).toBeVisible();

  await page.evaluate(() => {
    (window as any).__workdeckLoadingFlashes = [];
    const record = () => {
      const text = document.body.innerText;
      if (document.querySelector(".git-loading-shell")) {
        (window as any).__workdeckLoadingFlashes.push("git-skeleton");
      }
      if (text.includes("Preparing commit diff") || text.includes("Preparing pull request diff")) {
        (window as any).__workdeckLoadingFlashes.push("diff-preparation");
      }
    };
    new MutationObserver(record).observe(document.body, { childList: true, subtree: true });
  });

  await page.getByRole("tab", { name: /Pull requests/ }).click();
  await expect(page.locator(".task-changes")).toBeVisible();
  await page.getByRole("tab", { name: "Commits", exact: true }).click();
  await expect(page.locator(".git-row.is-selected")).toBeVisible();
  await expect(page.locator(".task-changes")).toBeVisible();
  await page.getByRole("tab", { name: /Pull requests/ }).click();
  await expect(page.locator(".task-changes")).toBeVisible();

  expect(await page.evaluate(() => (window as any).__workdeckLoadingFlashes)).toEqual([]);
});

test("refresh keeps the current commit and diff visible while revalidating", async ({ page }) => {
  await openWorkdeck(page, "/?fixture=polished&area=git");
  const selected = page.locator(".git-row.is-selected");
  await expect(selected).toBeVisible();
  const selectedOid = await selected.getAttribute("data-oid");
  await expect(page.locator(".task-changes")).toBeVisible();

  await page.getByRole("button", { name: "Refresh Git graph" }).click();
  await expect(page.locator(".git-loading-shell")).toHaveCount(0);
  await expect(page.locator(".task-changes")).toBeVisible();
  await expect(page.locator(".git-row.is-selected")).toHaveAttribute("data-oid", selectedOid ?? "");
});

test("pull request rows select real content and remain available after marking read", async ({ page }) => {
  await page.setViewportSize(viewports.wide);
  await openWorkdeck(page, "/?fixture=polished&area=pull-requests");
  const rows = page.locator(".pr-row");
  await expect(rows).toHaveCount(2);
  await rows.first().click();
  await expect(rows.first()).toHaveAttribute("aria-selected", "true");
  await expect(rows.first()).not.toHaveClass(/is-unread/);
  await expect(page.locator(".task-changes")).toBeVisible();
  await expect(page.getByRole("complementary", { name: "Changed files" })).toBeVisible();

  await page.getByRole("button", { name: "Filter pull requests" }).click();
  await page.getByRole("combobox", { name: "GitHub repository" }).selectOption("example/workdeck");
  await expect(page.locator(".task-diff-toolbar__context strong")).toHaveText("Harden repository discovery");
  await expect(page.locator(".pr-row")).toHaveAttribute("aria-selected", "true");
});

test("commit and reference rows open a diff with changed files", async ({ page }) => {
  await page.setViewportSize(viewports.wide);
  await openWorkdeck(page, "/?fixture=polished&area=git");
  const commit = page.getByRole("option", { name: /Refine opportunity ranking/ });
  await commit.click();
  await expect(commit).toHaveAttribute("aria-selected", "true");
  await expect(page.locator(".task-changes")).toBeVisible();
  await expect(page.locator(".task-diff-toolbar__context strong")).toHaveText("Refine opportunity ranking");
  await expect(page.getByRole("complementary", { name: "Changed files" })).toBeVisible();

  await page.locator("summary[aria-label='Browse Git references']").click();
  await page.getByRole("button", { name: /Open origin\/main at/ }).click();
  await expect(page.locator(".task-diff-toolbar__context small")).toContainText("5b71c02");
});

test("task-owned split panes resize by keyboard and pointer and can collapse", async ({ page }) => {
  await page.setViewportSize(viewports.wide);
  await openWorkdeck(page, "/?fixture=polished&area=pull-requests");

  const master = page.getByLabel("Pull request browser");
  const resizer = page.getByRole("separator", { name: "Resize pull request list" });
  await resizer.focus();
  const before = Number(await resizer.getAttribute("aria-valuenow"));
  await page.keyboard.press("ArrowRight");
  await expect(resizer).toHaveAttribute("aria-valuenow", String(before + 16));

  const start = await master.boundingBox();
  const handle = await resizer.boundingBox();
  expect(start).not.toBeNull();
  expect(handle).not.toBeNull();
  await page.mouse.move(handle!.x + handle!.width / 2, handle!.y + 80);
  await page.mouse.down();
  await page.mouse.move(handle!.x + 64, handle!.y + 80, { steps: 5 });
  await page.mouse.up();
  const expanded = await master.boundingBox();
  expect(expanded!.width).toBeGreaterThan(start!.width + 40);

  await page.getByRole("button", { name: "Hide pull request list" }).click();
  await expect(master).toHaveCount(0);
  await page.getByRole("button", { name: "Show pull request list" }).click();
  await expect(master).toBeVisible();

  const files = page.getByRole("separator", { name: "Resize changed files" });
  await files.focus();
  await page.keyboard.press("Home");
  await expect(files).toHaveAttribute("aria-valuenow", "220");
  await page.getByRole("button", { name: "Hide changed files" }).click();
  await expect(page.getByRole("complementary", { name: "Changed files" })).toHaveCount(0);
  await page.getByRole("button", { name: "Show changed files" }).click();
  await expect(page.getByRole("complementary", { name: "Changed files" })).toBeVisible();

  await page.getByRole("tab", { name: "Commits", exact: true }).click();
  const commits = page.getByRole("separator", { name: "Resize commit list" });
  await commits.focus();
  await page.keyboard.press("End");
  await expect(commits).toHaveAttribute("aria-valuenow", "640");

  await page.goto("/?fixture=polished&area=changes");
  await expect(page.locator(".status-bar")).toBeVisible();
  await expect(page.getByLabel("Changed files and symbols")).toBeVisible();
  await page.getByRole("button", { name: "Hide changed files" }).click();
  await expect(page.getByLabel("Changed files and symbols")).toHaveCount(0);
  await page.getByRole("button", { name: "Show changed files" }).click();
  await expect(page.getByLabel("Changed files and symbols")).toBeVisible();
  await page.getByRole("button", { name: "Hide canonical structure" }).click();
  await expect(page.getByLabel("Canonical tree")).toHaveCount(0);
  await page.getByRole("button", { name: "Show canonical structure" }).click();
  await expect(page.getByLabel("Canonical tree")).toBeVisible();
});

test("portfolio, CI, and artifact sidebars expose complete resize and collapse controls", async ({ page }) => {
  await page.setViewportSize(viewports.wide);
  await openWorkdeck(page, "/?fixture=polished&area=workspaces");
  const navigator = page.getByRole("separator", { name: "Resize navigator" });
  await navigator.focus();
  await page.keyboard.press("End");
  await expect(navigator).toHaveAttribute("aria-valuenow", "360");

  await page.getByRole("button", { name: "Toggle inspector" }).click();
  const inspector = page.getByRole("separator", { name: "Resize inspector" });
  await inspector.focus();
  await page.keyboard.press("Home");
  await expect(inspector).toHaveAttribute("aria-valuenow", "240");

  await page.getByRole("button", { name: "CI" }).click();
  await expect(page.getByLabel("CI runs")).toBeVisible();
  await page.getByRole("button", { name: "Hide CI run list" }).click();
  await expect(page.getByLabel("CI runs")).toHaveCount(0);
  await page.getByRole("button", { name: "Show CI run list" }).click();
  await expect(page.getByLabel("CI runs")).toBeVisible();
  const ciResize = page.getByRole("separator", { name: "Resize CI run list" });
  await ciResize.focus();
  await page.keyboard.press("Home");
  await expect(ciResize).toHaveAttribute("aria-valuenow", "300");

  await page.getByRole("button", { name: "Artifacts" }).click();
  const library = page.getByRole("listbox", { name: "Artifact library" });
  await expect(library).toBeVisible();
  await page.getByRole("button", { name: "Hide artifact library" }).click();
  await expect(library).toHaveCount(0);
  await page.getByRole("button", { name: "Show artifact library" }).click();
  await expect(library).toBeVisible();
  const artifactResize = page.getByRole("separator", { name: "Resize artifact library" });
  await artifactResize.focus();
  await page.keyboard.press("End");
  await expect(artifactResize).toHaveAttribute("aria-valuenow", "520");
});

test("PR selection opens its diff in place without losing the list", async ({ page }) => {
  await openWorkdeck(page, "/?fixture=polished&area=pull-requests");
  const browser = page.getByLabel("Pull request browser");
  await expect(browser).toBeVisible();
  await expect(page.locator(".task-changes")).toBeVisible();
  await expect(page.getByRole("complementary", { name: "Changed files" })).toBeVisible();
  await expect(page.locator(".pr-row.is-selected")).toHaveCount(1);
});

test("workspace shell uses one header row without duplicate global chrome", async ({ page }) => {
  await openWorkdeck(page, "/?fixture=polished&area=inbox");
  await expect(page.locator(".context-toolbar")).toHaveCount(0);
  await expect(page.locator(".titlebar")).toHaveCSS("height", "52px");
  await expect(page.locator(".app-rail__mark")).toHaveCount(0);
  await expect(page.locator(".viewport-tabs")).toHaveCount(0);
  await expect(page.locator(".navigator")).toHaveCount(0);
  await expect(page.locator(".inspector")).toHaveCount(0);
});

test("the unified macOS titlebar clears every rail and titlebar control", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 600 });
  await openWorkdeck(page, "/?fixture=polished&area=git");

  const shell = await page.locator(".workdeck-app").boundingBox();
  const titlebar = await page.locator(".titlebar").boundingBox();
  const rail = await page.locator(".app-rail").boundingBox();
  const titlebarSpacer = await page.locator(".titlebar__traffic-space").boundingBox();
  const firstRailControl = await page.locator(".app-rail__primary .rail-button").first().boundingBox();
  const firstTitlebarControl = await page.locator(".titlebar__context button").first().boundingBox();

  expect(shell).not.toBeNull();
  expect(titlebar).not.toBeNull();
  expect(rail).not.toBeNull();
  expect(titlebarSpacer).not.toBeNull();
  expect(firstRailControl).not.toBeNull();
  expect(firstTitlebarControl).not.toBeNull();
  expect(Math.abs(titlebar!.x - shell!.x)).toBeLessThan(0.5);
  expect(Math.abs(titlebar!.width - shell!.width)).toBeLessThan(0.5);
  expect(rail!.y).toBeGreaterThanOrEqual(titlebar!.y + titlebar!.height);
  expect(firstRailControl!.y).toBeGreaterThanOrEqual(titlebar!.y + titlebar!.height);
  expect(titlebarSpacer!.width).toBeGreaterThanOrEqual(80);
  expect(firstTitlebarControl!.x).toBeGreaterThanOrEqual(titlebarSpacer!.x + titlebarSpacer!.width);
});

test("PR and CI repository context is explicit and changes provider results", async ({ page }) => {
  await openWorkdeck(page, "/?fixture=polished&area=pull-requests");
  await page.getByRole("button", { name: "Filter pull requests" }).click();
  const repository = page.getByRole("combobox", { name: "GitHub repository" });
  await expect(repository).toHaveValue("");
  await expect(page.locator(".pr-list-scope")).toContainText("active repositories");
  await expect(page.getByRole("option", { name: "example/workdeck" })).toHaveCount(1);
  await repository.selectOption("example/workdeck");
  await expect(page.locator(".task-diff-toolbar__context strong")).toHaveText("Harden repository discovery");
  await expect(page.getByText("Feed-first dashboard and opportunity review")).toHaveCount(0);

  await page.getByRole("button", { name: "CI" }).click();
  await expect(page.getByRole("combobox", { name: "GitHub repository" })).toHaveValue("example/workdeck");
  await expect(page.getByText("No workflow runs for example/workdeck.")).toBeVisible();
});

test("minimum layout never overlays navigator and inspector", async ({ page }) => {
  await page.setViewportSize(viewports.minimum);
  await openWorkdeck(page, "/?fixture=polished&area=workspaces");
  await expect(page.locator(".navigator")).toBeVisible();
  await expect(page.locator(".inspector")).toHaveCount(0);
  await expect(page.locator(".workspace-table")).toBeVisible();
});

test("Git graph renders exact lane continuity, merge edges, HEAD, and WIP", async ({ page }) => {
  await openWorkdeck(page, "/?fixture=polished&area=git");
  const rows = page.locator(".git-row");
  await expect(rows).toHaveCount(7);
  await expect(rows.first()).toHaveClass(/is-wip/);
  await expect(rows.first().locator(".git-row__compact-meta").getByText("6 files")).toBeVisible();

  const head = page.getByRole("option", { name: /Refine opportunity ranking/ });
  await expect(head.locator(".graph-node--head")).toHaveCount(1);
  await expect(head.getByText("feat\/opportunities")).toBeVisible();

  const merge = page.getByRole("option", { name: /Merge main into feat\/opportunities/ });
  await expect(merge.locator(".graph-edge")).toHaveCount(2);
  await expect(merge.locator(".graph-edge[data-parent-oid]")).toHaveCount(2);
  await expect(merge.getByText("2 parents")).toBeVisible();
  const mergePaths = await merge.locator(".graph-edge").evaluateAll((edges) =>
    edges.map((edge) => edge.getAttribute("d"))
  );
  expect(mergePaths).toContain("M 14 24 L 14 48");
  expect(mergePaths.some((path) => path?.endsWith("28 48"))).toBe(true);
});

test("Git range selection normalizes base and head and excludes WIP", async ({ page }) => {
  await openWorkdeck(page, "/?fixture=polished&area=git");
  await page.getByRole("option", { name: /Working tree changes/ }).click();
  await page.getByRole("option", { name: /Feed-first dashboard baseline/ }).click();
  await page.getByRole("option", { name: /Refine opportunity ranking/ }).click({ modifiers: ["Shift"] });
  await expect(page.getByText(/Anchor f0a918d/)).toBeVisible();
  await expect(page.getByRole("button", { name: "Compare range" })).toBeVisible();
  await expect(page.locator(".git-row.is-in-range")).toHaveCount(6);
  await page.getByRole("button", { name: "Compare range" }).click();
  await expect(page.locator(".task-changes")).toBeVisible();
  await expect(page.getByRole("complementary", { name: "Changed files" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Git commit history" })).toBeVisible();
  await page.getByRole("button", { name: "Cancel commit range" }).click();
  await expect(page.locator(".git-row.is-in-range")).toHaveCount(0);
});

test("Git history supports bounded pagination, filtering, keyboard selection, and reference browsing", async ({ page }) => {
  await openWorkdeck(page, "/?fixture=polished&area=git");
  const history = page.getByRole("region", { name: "Git commit history" });
  await history.focus();
  await page.keyboard.press("Home");
  await expect(page.getByRole("option", { name: /Working tree changes/ })).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("ArrowDown");
  await expect(page.getByRole("option", { name: /Refine opportunity ranking/ })).toHaveAttribute("aria-selected", "true");

  await page.locator("summary[aria-label='Browse Git references']").click();
  await expect(page.locator("aside.git-references")).toBeVisible();
  await page.locator("summary[aria-label='Browse Git references']").click();
  await expect(page.locator("aside.git-references")).toBeHidden();

  await page.getByRole("button", { name: "Load older commits" }).click();
  await expect(page.locator(".git-row")).toHaveCount(10);
  await expect(page.getByRole("button", { name: "Load older commits" })).toHaveCount(0);
  const uniqueOids = await page.locator(".git-row").evaluateAll((rows) =>
    new Set(rows.map((row) => row.getAttribute("data-oid"))).size
  );
  expect(uniqueOids).toBe(10);

  await page.getByRole("searchbox", { name: "Filter commit history" }).fill("feat/opportunities");
  await expect(page.locator(".git-row")).toHaveCount(2);
  await expect(page.getByText("2 commits")).toBeVisible();
});

for (const surface of surfaces) {
  test(`axe contract: ${surface}`, async ({ page }) => {
    await openWorkdeck(page, `/?fixture=polished&area=${surface}`);
    const result = await new AxeBuilder({ page }).analyze();
    const blocking = result.violations.filter((violation) => violation.impact === "critical" || violation.impact === "serious");
    expect(blocking, JSON.stringify(blocking, null, 2)).toEqual([]);
  });
}

for (const [viewportName, viewport] of Object.entries(viewports)) {
  for (const theme of themes) {
    for (const surface of surfaces) {
      test(`visual ${surface} ${viewportName} ${theme}`, async ({ page }) => {
        await page.setViewportSize(viewport);
        await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
        await openWorkdeck(page, `/?fixture=polished&area=${surface}`);
        if (surface === "git" || surface === "pull-requests") {
          await expect(page.locator(".task-changes")).toBeVisible();
        }
        await expect(page).toHaveScreenshot(`matrix/${surface}-${viewportName}-${theme}.png`, { fullPage: true });
      });
    }
  }
}

for (const state of ["empty", "offline"] as const) {
  for (const theme of themes) {
    test(`visual state ${state} ${theme}`, async ({ page }) => {
      await page.setViewportSize(viewports.wide);
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await openWorkdeck(page, `/?fixture=${state}&area=inbox`);
      await expect(page).toHaveScreenshot(`states/${state}-${theme}.png`, { fullPage: true });
    });
  }
}

for (const viewportName of ["minimum", "wide"] as const) {
  for (const theme of themes) {
    test(`visual pull-request code in place ${viewportName} ${theme}`, async ({ page }) => {
      await page.setViewportSize(viewports[viewportName]);
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await openWorkdeck(page, "/?fixture=polished&area=pull-requests");
      await expect(page.locator(".task-changes")).toBeVisible();
      await expect(page).toHaveScreenshot(`interactions/pr-code-${viewportName}-${theme}.png`, { fullPage: true });
    });
  }
}

test("visual commit comparison preserves references", async ({ page }) => {
  await page.setViewportSize(viewports.wide);
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" });
  await openWorkdeck(page, "/?fixture=polished&area=git");
  await page.getByRole("option", { name: /Feed-first dashboard baseline/ }).click();
  await page.getByRole("option", { name: /Refine opportunity ranking/ }).click({ modifiers: ["Shift"] });
  await page.getByRole("button", { name: "Compare range" }).click();
  await expect(page.locator(".task-changes")).toBeVisible();
  await expect(page).toHaveScreenshot("interactions/git-range-in-place-wide-dark.png", { fullPage: true });
});

test("visual commit diff preserves history and changed-file context", async ({ page }) => {
  await page.setViewportSize(viewports.wide);
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" });
  await openWorkdeck(page, "/?fixture=polished&area=git");
  await page.getByRole("option", { name: /Refine opportunity ranking/ }).click();
  await expect(page.locator(".task-changes")).toBeVisible();
  await expect(page.getByRole("complementary", { name: "Changed files" })).toBeVisible();
  await expect(page).toHaveScreenshot("interactions/git-commit-inspector-wide-dark.png", { fullPage: true });
});

for (const theme of themes) {
  test(`visual component gallery ${theme}`, async ({ page }) => {
    await page.setViewportSize(viewports.wide);
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await page.goto("/?gallery=1");
    await expect(page.getByRole("heading", { name: "Workdeck component gallery" })).toBeVisible();
    await expect(page).toHaveScreenshot(`gallery/components-${theme}.png`, { fullPage: true });
  });
}
