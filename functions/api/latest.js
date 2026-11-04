// GET /api/latest：官网下载按钮的版本号和安装包大小（见 mirror/mirror.js）
import { latestInfo, onlyGet } from "../../mirror/mirror.js";
export const onRequest = (ctx) => onlyGet(ctx.request) || latestInfo(new URL(ctx.request.url), ctx.waitUntil.bind(ctx));
