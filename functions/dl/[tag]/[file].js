// GET /dl/<vX.Y.Z>/<文件名>：Release 附件镜像（见 mirror/mirror.js）
import { asset, onlyGet } from "../../../mirror/mirror.js";
export const onRequest = (ctx) => onlyGet(ctx.request) || asset(ctx.request, new URL(ctx.request.url), String(ctx.params.tag), String(ctx.params.file), ctx.waitUntil.bind(ctx));
