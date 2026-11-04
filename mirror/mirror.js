// 官网同域接口和更新镜像（Cloudflare Pages Functions，见仓库根目录 functions/；随 main 自动部署）。
// 只转发 hongfaqiu/blankrun 的 GitHub Release，不是通用代理：
//   GET /api/latest              → { version, size: { mac, win } }：官网下载按钮用（国内常连不上 api.github.com）
//   GET /update/latest.json      → GitHub 最新正式版的 latest.json，安装包地址改写成本域名 /dl/<tag>/<文件名>
//   GET /dl/<vX.Y.Z>/<文件名>    → 对应 Release 附件，边缘缓存 30 天（同一个 tag 的附件不会变）
// 安装包的 minisign 签名原样透传，App 用内置公钥校验，镜像无法篡改；镜像出错时 App 自动回到 GitHub。
export const REPO = "hongfaqiu/blankrun";
const GH = `https://github.com/${REPO}/releases`;
export const TAG = /^v\d+\.\d+\.\d+$/;
export const FILE = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;
const GH_ASSET = /^https:\/\/github\.com\/hongfaqiu\/blankrun\/releases\/download\/(v\d+\.\d+\.\d+)\/([A-Za-z0-9][A-Za-z0-9._-]{0,127})$/;
export const FILES = { mac: "BlankRun_universal.dmg", win: "BlankRun_x64-setup.exe" };

export function text(s, status, extra) {
  return new Response(s, { status, headers: { "content-type": "text/plain; charset=utf-8", "cache-control": "no-store", ...extra } });
}
function json(obj, maxAge) {
  return new Response(JSON.stringify(obj), {
    headers: { "content-type": "application/json; charset=utf-8", "cache-control": `public, max-age=${maxAge}`, "access-control-allow-origin": "*" },
  });
}
export function onlyGet(req) {
  return req.method === "GET" || req.method === "HEAD" ? null : text("method not allowed", 405, { allow: "GET, HEAD" });
}

/** latest.json：只改 platforms.*.url，别的字段（version、signature、notes…）原样返回 */
export function rewriteManifest(j, origin) {
  for (const p of Object.values((j && j.platforms) || {})) {
    const m = p && typeof p.url === "string" && p.url.match(GH_ASSET);
    if (m) p.url = `${origin}/dl/${m[1]}/${m[2]}`;
  }
  return j;
}

/** GitHub 上最新正式版的 latest.json（出错返回 null） */
async function upstreamManifest() {
  try {
    const r = await fetch(`${GH}/latest/download/latest.json`, { redirect: "follow", cf: { cacheEverything: true, cacheTtl: 300 } });
    if (!r.ok) return null;
    const j = await r.json();
    return j && typeof j.version === "string" && /^\d+\.\d+\.\d+$/.test(j.version) && j.platforms && typeof j.platforms === "object" ? j : null;
  } catch (e) { return null; }
}

export async function manifest(url) {
  const j = await upstreamManifest();
  return j ? json(rewriteManifest(j, url.origin), 300) : text("upstream unavailable", 502);
}

/** 官网用：最新版本号和两个安装包的大小（大小拿不到就不给，页面用写死的兜底） */
export async function latestInfo(url, waitUntil) {
  const key = new Request(`${url.origin}/api/latest`);
  const cache = caches.default;
  const hit = await cache.match(key);
  if (hit) return hit;
  const j = await upstreamManifest();
  if (!j) return text("upstream unavailable", 502);
  const tag = "v" + j.version, size = {};
  await Promise.all(Object.entries(FILES).map(async ([k, f]) => {
    try {
      const r = await fetch(`${GH}/download/${tag}/${f}`, { method: "HEAD", redirect: "follow" });
      const n = +(r.ok && r.headers.get("content-length"));
      if (n > 0) size[k] = n;
    } catch (e) {}
  }));
  const res = json({ version: j.version, tag, size }, 600);
  waitUntil(cache.put(key, res.clone()));
  return res;
}

export async function asset(req, url, tag, file, waitUntil) {
  if (!TAG.test(tag) || !FILE.test(file)) return text("not found", 404);
  // GitHub 会 302 到带签名参数的临时地址，所以按我们自己的 URL 存进缓存
  const key = new Request(`${url.origin}/dl/${tag}/${file}`);
  const cache = caches.default;
  let res = await cache.match(key);
  if (!res) {
    let up;
    try { up = await fetch(`${GH}/download/${tag}/${file}`, { redirect: "follow" }); }
    catch (e) { return text("upstream unreachable", 502); }
    if (!up.ok) return text(`upstream ${up.status}`, up.status === 404 ? 404 : 502);
    const h = new Headers();
    h.set("content-type", "application/octet-stream");
    h.set("content-disposition", `attachment; filename="${file}"`);
    h.set("cache-control", "public, max-age=2592000, immutable");
    const len = up.headers.get("content-length"); if (len) h.set("content-length", len);
    res = new Response(up.body, { status: 200, headers: h });
    waitUntil(cache.put(key, res.clone()));
  }
  return req.method === "HEAD" ? new Response(null, { status: res.status, headers: res.headers }) : res;
}
