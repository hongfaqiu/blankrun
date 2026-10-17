// 渲染设计稿 PNG（2x DPR）。用法：NODE_PATH=<含 playwright-core 的 node_modules> node render.mjs
// 浏览器用系统里的 Chrome；只截 .stage 元素，所以页面高度随内容走。
import { createRequire } from 'module';
const require = createRequire(import.meta.url);
const { chromium } = require('playwright-core');
import { fileURLToPath } from 'url'; import path from 'path'; import fs from 'fs';
const dir = path.dirname(fileURLToPath(import.meta.url)); fs.mkdirSync(path.join(dir, 'png'), { recursive: true });
const jobs = [
  ['popover.html?p=mac', 'popover-mac.png', 440],
  ['popover.html?p=mac&state=countdown', 'popover-mac-countdown.png', 440],
  ['popover.html?p=win', 'popover-win.png', 440],
  ...['general', 'blank', 'wake', 'schedule', 'about'].map(s => [`settings.html?p=mac#${s}`, `settings-mac-${s}.png`, 900]),
  ...['general', 'blank', 'wake', 'schedule', 'about'].map(s => [`settings.html?p=win#${s}`, `settings-win-${s}.png`, 1000]),
  ['overlay.html?p=mac&state=hint', 'blank-mac-hint.png', 1280],
  ['overlay.html?p=mac&state=hold', 'blank-mac-holding.png', 1280],
  ['overlay.html?p=win&state=hint', 'blank-win-hint.png', 1280],
  ['onboarding.html?p=mac', 'onboarding-mac.png', 700],
];
// 深色版（prefers-color-scheme: dark），输出 *-dark.png
const darkJobs = [
  ['popover.html?p=mac', 'popover-mac-dark.png', 440],
  ['popover.html?p=win', 'popover-win-dark.png', 440],
  ['settings.html?p=mac#wake', 'settings-mac-wake-dark.png', 900],
  ['settings.html?p=mac#blank', 'settings-mac-blank-dark.png', 900],
  ['onboarding.html?p=mac', 'onboarding-mac-dark.png', 700],
];
const only = process.argv[2];
const browser = await chromium.launch({ executablePath: '/usr/bin/google-chrome', args: ['--no-sandbox'] });
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 }, deviceScaleFactor: 2 });
const all = [...jobs.map(j => [...j, 'light']), ['overview.html', 'overview.png', 1800, 'light'],
             ...darkJobs.map(j => [...j, 'dark']), ['overview.html', 'overview-dark.png', 1800, 'dark']];
for (const [url, out, w, scheme] of all) {
  if (only && !out.includes(only)) continue;
  await page.emulateMedia({ colorScheme: scheme });
  await page.setViewportSize({ width: w, height: 1000 });
  await page.goto('file://' + path.join(dir, url)); await page.waitForLoadState('load'); await page.waitForTimeout(250);
  await page.locator('.stage').first().screenshot({ path: path.join(dir, 'png', out) });
  console.log('png/' + out);
}
await browser.close();
