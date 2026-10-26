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
  jobs.push([`popover.html?os=${os}`, `popover-${os}`, 320, 428]);
  jobs.push([`popover.html?os=${os}&state=countdown`, `popover-${os}-countdown`, 320, 428]);
  for (const s of ["general", "blank", "wake", "schedule", "about"]) jobs.push([`settings.html?os=${os}#${s}`, `settings-${os}-${s}`, 820, 760]);
}
jobs.push([`settings.html?os=mac&upd=available&from=0.1.0#about`, `settings-mac-about-upd`, 820, 1100]);
jobs.push([`settings.html?os=win&lang=en#about`, `settings-win-about-en`, 820, 1100]);
jobs.push([`popover.html?os=win&upd=available`, `popover-win-upd`, 320, 468]);
jobs.push([`popover.html?os=mac&state=blank&awake=off`, `popover-mac-noawake`, 320, 428]);
jobs.push([`settings.html?os=mac&mode=selected#blank`, `settings-mac-blank-selected`, 820, 760]);
jobs.push([`settings.html?os=win&mode=selected#blank`, `settings-win-blank-selected`, 820, 900]);
jobs.push([`settings.html?os=win&lang=en#blank`, `settings-win-blank-en`, 820, 900]);
jobs.push([`settings.html?os=mac&mode=selected&keep=1#blank`, `settings-mac-blank-keep`, 820, 900]);
jobs.push([`settings.html?os=mac&sched=multi#schedule`, `settings-mac-schedule-multi`, 820, 1100]);
jobs.push([`settings.html?os=win&sched=multi#schedule`, `settings-win-schedule-multi`, 820, 1100]);
jobs.push([`settings.html?os=mac&theme=dark#general`, `settings-mac-general-forced-dark`, 820, 760]);
jobs.push([`popover.html?os=mac&theme=light&sched=multi`, `popover-mac-forced-light`, 320, 428]);
jobs.push([`overlay.html?os=mac`, `overlay-mac-hint`, 1280, 800, { kind: "hint", reblank_s: 3 }]);
jobs.push([`overlay.html?os=win`, `overlay-win-hold`, 1280, 800, { kind: "hold", progress: 0.62 }]);
jobs.push([`onboarding.html?os=mac&perm=no`, `onboarding-mac`, 600, 600]);
// 英文版（配置里选 English）；所有 url 都带 ?
for (const [url, name, w, hgt, ov] of jobs.slice()) jobs.push([url.includes("#") ? url.replace("#", "&lang=en#") : url + "&lang=en", name + "-en", w, hgt, ov]);
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
