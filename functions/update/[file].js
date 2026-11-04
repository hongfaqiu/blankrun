// GET /update/latest.json：App 内更新的镜像清单（见 mirror/mirror.js）
import { manifest, onlyGet, text } from "../../mirror/mirror.js";
export const onRequest = (ctx) => onlyGet(ctx.request) || (ctx.params.file === "latest.json" ? manifest(new URL(ctx.request.url)) : text("not found", 404));
