// node tools/update-mirror/test.mjs：不联网，模拟 fetch / caches 检查改写和路由
import assert from "node:assert/strict";
import worker, { rewriteManifest } from "./src/index.js";
const gh = "https://github.com/hongfaqiu/blankrun/releases/download/v0.1.11/";
const man = { version: "0.1.11", notes: "x", platforms: {
  "darwin-aarch64": { signature: "SIG", url: gh + "BlankRun_universal.app.tar.gz" },
  "windows-x86_64": { signature: "SIG2", url: gh + "BlankRun_0.1.11_x64-setup.exe" },
  "evil": { signature: "S", url: "https://evil.example/x.tar.gz" } } };
const out = rewriteManifest(structuredClone(man), "https://blankrun.cosfoc.com");
assert.equal(out.platforms["darwin-aarch64"].url, "https://blankrun.cosfoc.com/dl/v0.1.11/BlankRun_universal.app.tar.gz");
assert.equal(out.platforms["darwin-aarch64"].signature, "SIG");
assert.equal(out.platforms["windows-x86_64"].url, "https://blankrun.cosfoc.com/dl/v0.1.11/BlankRun_0.1.11_x64-setup.exe");
assert.equal(out.platforms.evil.url, "https://evil.example/x.tar.gz");
const seen = [];
globalThis.fetch = async (u) => { seen.push(String(u)); if (String(u).endsWith("latest.json")) return new Response(JSON.stringify(man)); if (String(u).endsWith("/download/v0.1.11/BlankRun_universal.app.tar.gz")) return new Response("BYTES", { headers: { "content-length": "5" } }); return new Response("nf", { status: 404 }); };
const store = new Map();
globalThis.caches = { default: { match: async (k) => store.get(k.url)?.clone(), put: async (k, r) => { store.set(k.url, r); } } };
const ctx = { waitUntil: (p) => p };
const get = (p, m = "GET") => worker.fetch(new Request("https://blankrun.cosfoc.com" + p, { method: m }), {}, ctx);
let r = await get("/update/latest.json"); assert.equal(r.status, 200);
assert.equal((await r.json()).platforms["darwin-aarch64"].url, "https://blankrun.cosfoc.com/dl/v0.1.11/BlankRun_universal.app.tar.gz");
r = await get("/dl/v0.1.11/BlankRun_universal.app.tar.gz"); assert.equal(r.status, 200); assert.equal(await r.text(), "BYTES");
assert.equal((await get("/dl/v0.1.11/..")).status, 404);
assert.equal((await get("/dl/main/x.zip")).status, 404);
assert.equal((await get("/dl/v0.1.11/missing.zip")).status, 404);
assert.equal((await get("/other")).status, 404);
assert.equal((await get("/update/latest.json", "POST")).status, 405);
assert.ok(seen.every((u) => u.startsWith("https://github.com/hongfaqiu/blankrun/releases/")));
console.log("update-mirror worker: ok");
