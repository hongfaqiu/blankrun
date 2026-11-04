# 更新镜像（Cloudflare Worker）

给 App 内更新和官网下载用的镜像，挂在 `blankrun.cosfoc.com` 自己的域名下：

| 路径 | 作用 | 缓存 |
| --- | --- | --- |
| `GET /update/latest.json` | 取 GitHub 最新正式版的 `latest.json`，把安装包地址改写成 `/dl/<tag>/<文件名>`，签名等字段原样保留 | 5 分钟 |
| `GET /dl/<vX.Y.Z>/<文件名>` | 转发 `hongfaqiu/blankrun` 对应 Release 的附件 | 边缘缓存 30 天（tag 的附件不变） |

只转发这一个仓库、只认 `vX.Y.Z` 形式的 tag 和简单文件名，不是开放代理。

**安全**：App 下载后用 `src-tauri/tauri.conf.json` 里内置的 minisign 公钥校验签名，镜像就算被篡改也装不上；
镜像连不上、返回非 2xx 或内容不对时，App 自动改回 GitHub（`src-tauri/src/update.rs`）。

**为什么用自己的 Worker 而不是 ghproxy 一类公共镜像**：公共镜像由个人维护，经常被墙、限速、换域名或停服，
还要把下载交给第三方；自己的 Worker 在自己的 Cloudflare 账号和域名下，可控、可随时下线，和官网同一个已经能访问的域名。
完整性由签名保证，所以镜像只需要解决「能不能下得动」。

## 部署（需要仓库主人确认后手动执行，本 PR 没有部署）

前提：`cosfoc.com` 这个 zone 在同一个 Cloudflare 账号里（NS 已是 Cloudflare），`blankrun.cosfoc.com` 已经由 Pages 提供官网。

```bash
cd tools/update-mirror
node test.mjs                 # 本地自测（不联网）
npx wrangler login            # 用拥有 cosfoc.com 的账号登录
npx wrangler deploy           # 发布 Worker，并按 wrangler.toml 绑定两条路由：
                              #   blankrun.cosfoc.com/update/*  和  blankrun.cosfoc.com/dl/*
```

路由只覆盖 `/update/*` 和 `/dl/*`，官网其它路径仍由 Pages 提供。

部署后验证：

```bash
curl -s https://blankrun.cosfoc.com/update/latest.json | head -c 400   # url 应该是 https://blankrun.cosfoc.com/dl/v…/…
curl -sI https://blankrun.cosfoc.com/dl/v0.1.9/BlankRun_0.1.9_x64-setup.exe   # 200，content-length 和 GitHub 上一致
```

回滚：`npx wrangler delete`（或在 Cloudflare 后台删掉这两条路由）。删掉后 `/update/latest.json` 回到 404，
App 自动走 GitHub，官网下载按钮也自动回到 GitHub 链接（`site/dl.js` 会先探测镜像）。

用量：免费版每天 10 万次请求；安装包只有几 MB，缓存命中后不再回源 GitHub。
