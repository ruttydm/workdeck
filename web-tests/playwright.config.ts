import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  outputDir: "../artifacts/playwright",
  snapshotPathTemplate: "{testDir}/__screenshots__/{arg}{ext}",
  timeout: 30_000,
  expect: { timeout: 8_000, toHaveScreenshot: { animations: "disabled", caret: "hide", maxDiffPixelRatio: 0.003 } },
  fullyParallel: false,
  workers: 1,
  reporter: [["line"]],
  use: {
    baseURL: "http://127.0.0.1:4173",
    browserName: "chromium",
    locale: "en-US",
    timezoneId: "UTC",
    reducedMotion: "reduce",
    trace: "retain-on-failure"
  },
  webServer: {
    command: "python3 -m http.server 4173 --bind 127.0.0.1 --directory ../target/dx/workdeck-web/debug/web/public 2>/dev/null",
    url: "http://127.0.0.1:4173",
    reuseExistingServer: false,
    timeout: 15_000
  }
});
