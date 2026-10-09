//! 构建脚本：用 windres 把 assets/app.rc 编译成 COFF 对象，
//! 再作为链接参数交给 rustc，从而把自定义图标嵌进 exe。
//!
//! windres 来自 MinGW-w64（build.bat 会把它的 bin 目录加进 PATH）。
//! 如果找不到 windres，只打印警告，不影响编译 —— 只是 exe 没有自定义图标。

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/AfterburnerWebMonitor.ico");

    let out_dir = match std::env::var("OUT_DIR") {
        Ok(v) => PathBuf::from(v),
        Err(_) => {
            println!("cargo:warning=OUT_DIR 未设置，跳过图标嵌入");
            return;
        }
    };

    let obj = out_dir.join("app_icon.o");

    let result = Command::new("windres")
        .arg("assets/app.rc")
        .arg("-O")
        .arg("coff")
        .arg("-o")
        .arg(&obj)
        .status();

    match result {
        Ok(st) if st.success() && obj.exists() => {
            println!("cargo:rustc-link-arg={}", obj.display());
        }
        Ok(st) => {
            println!(
                "cargo:warning=windres 退出码 {:?}，图标未嵌入（确认 PATH 里有 MinGW 的 windres）",
                st.code()
            );
        }
        Err(e) => {
            println!("cargo:warning=无法执行 windres（{}），图标未嵌入", e);
        }
    }
}
