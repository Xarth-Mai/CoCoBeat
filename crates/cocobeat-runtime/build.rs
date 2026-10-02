use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=COCOBEAT_BUILD_ID");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads/main");
    println!("cargo:rerun-if-changed=src");
    let identity = std::env::var("COCOBEAT_BUILD_ID").unwrap_or_else(|_| {
        let revision = Command::new("git")
            .args(["rev-parse", "--short=12", "HEAD"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .unwrap_or_else(|| "source-archive".into());
        format!("{}-{revision}-local", env!("CARGO_PKG_VERSION"))
    });
    println!("cargo:rustc-env=COCOBEAT_BUILD_ID={identity}");
}
