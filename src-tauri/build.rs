// 扫描 ui/i18n/*.js，把可用语言列表（代码=显示名）交给编译期：新增语言只需要加一个文件。
fn main() {
    println!("cargo:rerun-if-changed=../ui/i18n");
    let mut list = Vec::new();
    if let Ok(rd) = std::fs::read_dir("../ui/i18n") {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("js") { continue; }
            let code = p.file_stem().and_then(|x| x.to_str()).unwrap_or("").to_string();
            println!("cargo:rerun-if-changed={}", p.display());
            let src = std::fs::read_to_string(&p).unwrap_or_default();
            // 显示名取文件里的 "_name": "..."（用该语言自己的写法，例如「简体中文」）
            let name = src.split("\"_name\"").nth(1)
                .and_then(|r| r.split('"').nth(1)).unwrap_or(&code).to_string();
            assert!(!code.is_empty() && !code.contains([';', '=']) && !name.contains([';', '=']), "bad locale file {}", p.display());
            list.push(format!("{code}={name}"));
        }
    }
    list.sort();
    println!("cargo:rustc-env=BLANKRUN_LOCALES={}", list.join(";"));
    tauri_build::build()
}
