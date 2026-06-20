use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.manifest");
    let out = std::env::var("OUT_DIR").unwrap();
    let obj = format!("{}/app_res.o", out);
    // コモンコントロールv6マニフェストを埋め込む (windresが無い環境でもビルドは継続)
    match Command::new("windres").args(["app.rc", "-O", "coff", "-o", &obj]).status() {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg={}", obj),
        _ => println!("cargo:warning=windres not found; manifest not embedded"),
    }
}
