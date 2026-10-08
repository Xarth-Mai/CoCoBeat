use std::{env, path::Path};

fn main() {
    let target = env::var("TARGET").expect("Cargo TARGET");
    assert!(
        matches!(
            target.as_str(),
            "x86_64-unknown-linux-gnu"
                | "aarch64-unknown-linux-gnu"
                | "x86_64-pc-windows-msvc"
                | "aarch64-pc-windows-msvc"
        ),
        "BTT candidate supports only the four declared GNU Linux / MSVC Windows targets"
    );
    let upstream = Path::new("vendor/btt");
    println!("cargo:rerun-if-changed=vendor/btt");
    println!("cargo:rerun-if-changed=src/shim.c");
    let mut build = cc::Build::new();
    build.include(upstream).define("_USE_MATH_DEFINES", None);
    if target.ends_with("-msvc") {
        // Upstream C99 VLAs require clang-cl; the resulting archive uses the MSVC ABI
        build.prefer_clang_cl_over_msvc(true);
        assert!(
            build.get_compiler().is_like_clang_cl(),
            "BTT requires the Visual Studio C++ Clang compiler component on Windows"
        );
        build.flag("/clang:-std=c11");
    } else {
        build.std("gnu99");
    }
    for file in ["BTT", "DFT", "STFT", "Filter", "Statistics", "fastsin"] {
        build.file(upstream.join("src").join(format!("{file}.c")));
    }
    build.file("src/shim.c").compile("cocobeat_btt_48000");
    if !target.ends_with("-msvc") {
        println!("cargo:rustc-link-lib=m");
    }
}
