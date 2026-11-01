use std::path::PathBuf;
use std::process::Command;

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    let ext = if target.contains("windows") { ".exe" } else { "" };
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let ffmpeg_sidecar = manifest_dir.join(format!("binaries/ffmpeg-{target}{ext}"));
    let ffprobe_sidecar = manifest_dir.join(format!("binaries/ffprobe-{target}{ext}"));

    if !ffmpeg_sidecar.exists() || !ffprobe_sidecar.exists() {
        println!("cargo:warning=Sidecar binaries for target '{target}' missing. Checking prepare-binaries script...");
        let script = manifest_dir.join("../../scripts/prepare-binaries.js");
        if script.exists() {
            let status = Command::new("node").arg(&script).status();
            if let Ok(st) = status {
                if !st.success() {
                    println!("cargo:warning=prepare-binaries.js exited with non-zero status");
                }
            }
        }
    }

    tauri_build::build()
}

