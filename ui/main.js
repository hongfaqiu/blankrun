// 无框架、无打包：直接用 withGlobalTauri 注入的 window.__TAURI__
const { invoke } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);
invoke("get_config").then((t) => ($("cfg").value = t));
$("save").onclick = async () => {
  try { await invoke("save_config", { toml: $("cfg").value }); $("msg").textContent = "已保存"; }
  catch (e) { $("msg").textContent = "错误：" + e; }
};
