// 用假的 __TAURI__ 在 Chrome 里渲染正式 ui/ 页面，检查布局（不是设计稿）。
// 用法：NODE_PATH=/tmp/pw/node_modules node design/preview/render.mjs  → design/preview/out/*.png
import { createRequire } from "module";
import { fileURLToPath } from "url"; import path from "path"; import fs from "fs";
const require = createRequire(import.meta.url);
const { chromium } = require("playwright-core");
const dir = path.dirname(fileURLToPath(import.meta.url));
const ui = path.resolve(dir, "../../ui");
const out = path.join(dir, "out"); fs.mkdirSync(out, { recursive: true });
const mock = fs.readFileSync(path.join(dir, "mock-tauri.js"), "utf8");
const jobs = [];
for (const os of ["mac", "win"]) {
  jobs.push([`popover.html?os=${os}`, `popover-${os}`, 320, 452]);
  jobs.push([`popover.html?os=${os}&state=countdown`, `popover-${os}-countdown`, 320, 452]);
  for (const s of ["general", "blank", "wake", "schedule", "about"]) jobs.push([`settings.html?os=${os}#${s}`, `settings-${os}-${s}`, 820, 760]);
}
jobs.push([`settings.html?os=mac&mode=selected#blank`, `settings-mac-blank-selected`, 820, 760]);
jobs.push([`overlay.html?os=mac`, `overlay-mac-hint`, 1280, 800, { kind: "hint", reblank_s: 3 }]);
jobs.push([`overlay.html?os=win`, `overlay-win-hold`, 1280, 800, { kind: "hold", progress: 0.62 }]);
jobs.push([`onboarding.html?os=mac&perm=no`, `onboarding-mac`, 600, 600]);
const browser = await chromium.launch({ executablePath: "/usr/bin/google-chrome", args: ["--no-sandbox", "--allow-file-access-from-files"] });
const errors = [];
for (const scheme of ["light", "dark"]) {
  for (const [url, name, w, hgt, overlay] of jobs) {
    const page = await browser.newPage({ viewport: { width: w, height: hgt }, deviceScaleFactor: 2, colorScheme: scheme });
    page.on("pageerror", (e) => errors.push(`${name}: ${e.message}`));
    page.on("console", (m) => { if (m.type() === "error") errors.push(`${name}: ${m.text()}`); });
    await page.addInitScript(mock);
    await page.goto("file://" + path.join(ui, url));
    await page.waitForTimeout(300);
    if (overlay) { await page.evaluate((p) => window.__emit("nb://overlay", p), overlay); await page.waitForTimeout(100); }
    await page.screenshot({ path: path.join(out, `${name}${scheme === "dark" ? "-dark" : ""}.png`), fullPage: false });
    await page.close();
  }
}
await browser.close();
console.log(errors.length ? "ERRORS:\n" + errors.join("\n") : "no page errors");
