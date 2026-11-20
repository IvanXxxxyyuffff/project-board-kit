// 构建脚本：用 MinGW 的 windres 把 assets/app.rc（图标 + 版本信息）编成 COFF 对象，
// 再作为链接参数塞进 exe。不引入 winres/embed-resource 之类的构建期 crate，
// 依赖树保持 14 个 crate 不变。
//
// 找不到 windres 时只打印 warning 并跳过（程序运行时会退化为读同目录 ico）。

use std::path::PathBuf;
use std::process::Command;

fn find_windres() -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("WINDRES") {
        cands.push(PathBuf::from(p));
    }
    cands.push(PathBuf::from(r"<TOOLS>\mingw64\bin\windres.exe"));
    for c in ["windres.exe", "x86_64-w64-mingw32-windres.exe"] {
        if let Ok(path) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path) {
                cands.push(dir.join(c));
            }
        }
    }
    cands.into_iter().find(|p| p.is_file())
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let assets = manifest.join("assets");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let obj = out.join("app.res.o");

    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-env-changed=WINDRES");

    let Some(windres) = find_windres() else {
        println!("cargo:warning=windres not found; icon/version resource skipped");
        return;
    };

    // 注意：windres 对非 ASCII 路径会按 ANSI 解码（本项目路径含中文），
    // 直接把 -o 指向 OUT_DIR 会报 "No such file or directory"。
    // 因此在 assets 目录内用相对文件名产出，再由 Rust 复制到 OUT_DIR。
    let tmp_name = "app.res.o";
    let tmp = assets.join(tmp_name);
    let _ = std::fs::remove_file(&tmp);

    let status = Command::new(&windres)
        .current_dir(&assets)
        .arg("app.rc")
        .arg("-O")
        .arg("coff")
        .arg("-o")
        .arg(tmp_name)
        .status();

    match status {
        Ok(s) if s.success() && tmp.is_file() => match std::fs::copy(&tmp, &obj) {
            Ok(_) => {
                println!("cargo:rustc-link-arg-bins={}", obj.display());
            }
            Err(e) => println!(
                "cargo:warning=copy resource object to OUT_DIR failed: {e}; resource skipped"
            ),
        },
        Ok(s) => {
            println!("cargo:warning=windres failed (code {s}); resource skipped");
        }
        Err(e) => {
            println!("cargo:warning=windres spawn failed: {e}; resource skipped");
        }
    }
    let _ = std::fs::remove_file(&tmp);
}
