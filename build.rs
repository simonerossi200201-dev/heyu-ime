use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/heyu.rc");
    println!("cargo:rerun-if-changed=assets/heyu.ico");
    println!("cargo:rerun-if-changed=assets/tray_light.ico");
    println!("cargo:rerun-if-changed=assets/tray_dark.ico");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let assets_dir = manifest_dir.join("assets");
    let rc_file = assets_dir.join("heyu.rc");
    if !rc_file.exists() {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let obj_file = out_dir.join("heyu_res.o");
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let windres = if arch == "x86" {
        "i686-w64-mingw32-windres"
    } else {
        "x86_64-w64-mingw32-windres"
    };

    if let Ok(status) = Command::new(windres)
        .arg("-I")
        .arg(&assets_dir)
        .arg(&rc_file)
        .arg("-O")
        .arg("coff")
        .arg("-o")
        .arg(&obj_file)
        .status()
    {
        if status.success() && obj_file.exists() {
            println!("cargo:rustc-link-arg-bin=heyu-ime={}", obj_file.display());
            println!("cargo:rustc-link-arg-bin=heyu-voice={}", obj_file.display());
            println!("cargo:rustc-link-arg-bin=heyu-polish={}", obj_file.display());
        }
    }
}
