import { expect, test } from "@playwright/test";
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

type Surface = "inbox" | "workspaces" | "git" | "search" | "artifacts";

const surfaceSelectors: Record<Surface, string> = {
  inbox: ".inbox-surface",
  workspaces: ".workspace-surface",
  git: ".git-surface",
  search: ".search-surface",
  artifacts: ".artifacts-surface"
};

test("74/75/109 fixture stays within renderer performance budgets", async ({ browser }) => {
  const runs: Array<Record<string, number>> = [];

  for (let iteration = 0; iteration < 3; iteration += 1) {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
    const page = await context.newPage();
    const launchStarted = performance.now();
    await page.goto("/?fixture=polished&area=inbox");
    await page.locator(".status-bar").waitFor({ state: "visible" });
    const usableLaunchMs = performance.now() - launchStarted;
    const inboxStarted = performance.now();
    await page.locator(".inbox-row").first().waitFor({ state: "visible" });
    const firstInboxMs = performance.now() - inboxStarted;

    const areaSwitchSamples: number[] = [];
    for (const surface of ["workspaces", "git", "search", "artifacts", "inbox"] as const) {
      const label = surface === "inbox"
        ? "Updates"
        : surface === "git"
          ? "Git"
          : `${surface[0].toUpperCase()}${surface.slice(1)}`;
      areaSwitchSamples.push(await page.evaluate(async ({ next, label, selector }) => {
        const button = document.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
        if (!button) throw new Error(`missing ${next} rail control`);
        const rendered = new Promise<void>((resolveRendered) => {
          if (document.querySelector(selector)) {
            resolveRendered();
            return;
          }
          const observer = new MutationObserver(() => {
            if (document.querySelector(selector)) {
              observer.disconnect();
              resolveRendered();
            }
          });
          observer.observe(document.body, { childList: true, subtree: true });
        });
        const started = performance.now();
        button.click();
        await rendered;
        await new Promise<void>((resolveFrame) => requestAnimationFrame(() => resolveFrame()));
        return performance.now() - started;
      }, { next: surface, label, selector: surfaceSelectors[surface] }));
    }

    const diffStarted = performance.now();
    await page.goto("/?fixture=polished&area=changes");
    await page.locator(".review-workbench").waitFor({ state: "visible" });
    const firstDiffMs = performance.now() - diffStarted;
    const sortedSwitches = [...areaSwitchSamples].sort((left, right) => left - right);
    const areaSwitchP95Ms = sortedSwitches[Math.ceil(sortedSwitches.length * 0.95) - 1];

    const run = {
      usable_launch_ms: Number(usableLaunchMs.toFixed(2)),
      first_inbox_ms: Number(firstInboxMs.toFixed(2)),
      area_switch_p95_ms: Number(areaSwitchP95Ms.toFixed(2)),
      first_diff_ms: Number(firstDiffMs.toFixed(2))
    };
    runs.push(run);
    expect(run.usable_launch_ms).toBeLessThanOrEqual(1200);
    expect(run.first_inbox_ms).toBeLessThanOrEqual(300);
    expect(run.area_switch_p95_ms).toBeLessThanOrEqual(50);
    expect(run.first_diff_ms).toBeLessThanOrEqual(250);
    await context.close();
  }

  const outputDirectory = resolve(__dirname, "../../artifacts/performance");
  mkdirSync(outputDirectory, { recursive: true });
  writeFileSync(resolve(outputDirectory, "web-renderer.json"), `${JSON.stringify({
    schema: 1,
    fixture: { projects: 74, repositories: 75, worktrees: 109 },
    runs
  }, null, 2)}\n`);
});
