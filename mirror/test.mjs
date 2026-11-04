// node mirror/test.mjs：不联网，模拟 fetch / caches，检查 Pages Functions 的路由、改写和拒绝
import assert from "node:assert/strict";
import { rewriteManifest } from "./mirror.js";
import * as latestFn from "../functions/api/latest.js";
import * as updFn from "../functions/update/[file].js";
import * as dlFn from "../functions/dl/[tag]/[file].js";
const gh = "https://github.com/hongfaqiu/blankrun/releases/download/v0.1.11/";
const man = { version: "0.1.11", notes: "x", platforms: {
  "darwin-aarch64": { signature: "SIG", url: gh + "BlankRun_0.1.11_universal.app.tar.gz" },
  "windows-x86_64": { signature: "SIG2", url: gh + "BlankRun_0.1.11_x64-setup.exe" },
  "evil": { signature: "S", url: "https://evil.example/x.tar.gz" } } };
const out = rewriteManifest(structuredClone(man), "https://blankrun.cosfoc.com");
assert.equal(out.platforms["darwin-aarch64"].url, "https://blankrun.cosfoc.com/dl/v0.1.11/BlankRun_0.1.11_universal.app.tar.gz");
assert.equal(out.platforms["darwin-aarch64"].signature, "SIG");
assert.equal(out.platforms.evil.url, "https://evil.example/x.tar.gz");
const seen = [];
globalThis.fetch = async (u, o = {}) => {
  u = String(u); seen.push(u);
  if (u.endsWith("/latest/download/latest.json")) return new Response(JSON.stringify(man));
  if (u.endsWith("/download/v0.1.11/BlankRun_universal.dmg")) return new Response(o.method === "HEAD" ? null : "D", { headers: { "content-length": "3984588" } });
  if (u.endsWith("/download/v0.1.11/BlankRun_x64-setup.exe")) return new Response(null, { headers: { "content-length": "1468006" } });
  if (u.endsWith("/download/v0.1.11/BlankRun_0.1.11_universal.app.tar.gz")) return new Response("BYTES", { headers: { "content-length": "5" } });
  return new Response("nf", { status: 404 });
};
const store = new Map();
globalThis.caches = { default: { match: async (k) => store.get(k.url)?.clone(), put: async (k, r) => { store.set(k.url, r); } } };
const call = (fn, path, params, method = "GET") => fn.onRequest({ request: new Request("https://blankrun.cosfoc.com" + path, { method }), params, waitUntil: (p) => p });
let r = await call(updFn, "/update/latest.json", { file: "latest.json" }); assert.equal(r.status, 200);
assert.equal((await r.json()).platforms["windows-x86_64"].url, "https://blankrun.cosfoc.com/dl/v0.1.11/BlankRun_0.1.11_x64-setup.exe");
assert.equal((await call(updFn, "/update/x.json", { file: "x.json" })).status, 404);
assert.equal((await call(updFn, "/update/latest.json", { file: "latest.json" }, "POST")).status, 405);
r = await call(latestFn, "/api/latest", {}); assert.equal(r.status, 200);
assert.deepEqual(await r.json(), { version: "0.1.11", tag: "v0.1.11", size: { mac: 3984588, win: 1468006 } });
r = await call(dlFn, "/dl/v0.1.11/BlankRun_0.1.11_universal.app.tar.gz", { tag: "v0.1.11", file: "BlankRun_0.1.11_universal.app.tar.gz" });
assert.equal(r.status, 200); assert.equal(await r.text(), "BYTES");
assert.equal((await call(dlFn, "/dl/main/x.zip", { tag: "main", file: "x.zip" })).status, 404);
assert.equal((await call(dlFn, "/dl/v0.1.11/..", { tag: "v0.1.11", file: ".." })).status, 404);
assert.equal((await call(dlFn, "/dl/v0.1.11/missing.zip", { tag: "v0.1.11", file: "missing.zip" })).status, 404);
assert.ok(seen.every((u) => u.startsWith("https://github.com/hongfaqiu/blankrun/releases/")));
console.log("site functions / mirror: ok");
