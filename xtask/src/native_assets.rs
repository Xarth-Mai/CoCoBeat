use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
};

fn checked(path: &Path, bytes: u64, digest: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.file_type().is_file() || metadata.len() != bytes {
        return Err(format!(
            "Wrong native resource type/size: {}",
            path.display()
        ));
    }
    let mut hash = blake3::Hasher::new();
    hash.update_reader(File::open(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if hash.finalize().to_hex().as_str() != digest {
        return Err(format!(
            "Wrong native resource identity: {}",
            path.display()
        ));
    }
    Ok(())
}

fn copy_new(source: &Path, destination: &Path) -> Result<(), String> {
    if !fs::symlink_metadata(source)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_file()
    {
        return Err(format!("Expected ordinary resource: {}", source.display()));
    }
    let mut input = File::open(source).map_err(|e| e.to_string())?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|e| e.to_string())?;
    io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn prepare(target: &str, sdk: &Path, package: &Path) -> Result<(), String> {
    // Fixed official SDK identities, independently measured from all four archives
    let (core, core_bytes, core_hash, provider, provider_bytes, provider_hash) = match target {
        "x86_64-pc-windows-msvc" => (
            "onnxruntime.dll",
            16462648,
            "fe8d52509b357b5822d452efe33dbec7f4b46d073868d558f0b90ab528f3d361",
            "onnxruntime_providers_shared.dll",
            21816,
            "7bd740640726050f663d6084b83c4bf26f7059bc015b91791dc331af00b1c4c5",
        ),
        "aarch64-pc-windows-msvc" => (
            "onnxruntime.dll",
            16728888,
            "6dd3cd272c0f599b301275b070c629959461185ff0a194653975bb662cd9861d",
            "onnxruntime_providers_shared.dll",
            21304,
            "d46a67bcd7401ef346725df7796c1e7062359685ca865df53b9fcd62b775f1f9",
        ),
        "x86_64-unknown-linux-gnu" => (
            "libonnxruntime.so.1.30.0",
            28985152,
            "1237b3b4730080f918c40b05f5553875e928df450eebae760cf6a4a883a72409",
            "libonnxruntime_providers_shared.so",
            14632,
            "e575eac21c52ff35f67d0179df4f76f0a953638a3448b982cf106112501df6b3",
        ),
        "aarch64-unknown-linux-gnu" => (
            "libonnxruntime.so.1.30.0",
            25135496,
            "b6ea7ae49606a8c3ac54353306ef665a7ad66775542b60c0663e0e85307215a7",
            "libonnxruntime_providers_shared.so",
            198792,
            "cb0e963677b9cc40f03d02fa20274e336711024ac47cf4371f7ede033ff7c258",
        ),
        _ => return Err(format!("Unsupported native SDK target: {target}")),
    };
    let model = Path::new("assets/models/beat-this/small0.onnx");
    let model_hash = "8fef35421f1babeec49fa74e4601a708048cbe1af9547fdab3d0a5368d577a90";
    checked(&sdk.join("lib").join(core), core_bytes, core_hash)?;
    checked(
        &sdk.join("lib").join(provider),
        provider_bytes,
        provider_hash,
    )?;
    checked(model, 10555597, model_hash)?;
    let library = package.join("lib/onnxruntime");
    let models = package.join("assets/models/beat-this");
    let notices = package.join("licenses/onnxruntime/sdk");
    for directory in [&library, &models, &notices] {
        fs::create_dir_all(directory.parent().ok_or("Resource has no parent")?)
            .map_err(|e| e.to_string())?;
        fs::create_dir(directory).map_err(|e| e.to_string())?;
    }
    copy_new(&sdk.join("lib").join(core), &library.join(core))?;
    copy_new(&sdk.join("lib").join(provider), &library.join(provider))?;
    if target.ends_with("linux-gnu") {
        #[cfg(unix)]
        for link in ["libonnxruntime.so.1", "libonnxruntime.so"] {
            std::os::unix::fs::symlink(core, library.join(link)).map_err(|e| e.to_string())?;
        }
        #[cfg(not(unix))]
        return Err("Prepare Linux SDK links on the native Linux runner".into());
    }
    for name in [
        "LICENSE",
        "ThirdPartyNotices.txt",
        "VERSION_NUMBER",
        "GIT_COMMIT_ID",
    ] {
        copy_new(&sdk.join(name), &notices.join(name))?;
    }
    for name in ["small0.onnx", "PROVENANCE.json", "README.md"] {
        copy_new(
            &Path::new("assets/models/beat-this").join(name),
            &models.join(name),
        )?;
    }
    checked(&library.join(core), core_bytes, core_hash)?;
    checked(&library.join(provider), provider_bytes, provider_hash)?;
    checked(&models.join("small0.onnx"), 10555597, model_hash)?;
    let receipt = serde_json::json!({"target": target, "sdk_version": "1.30.0", "core": {"path": format!("lib/onnxruntime/{core}"), "bytes": core_bytes, "blake3": core_hash}, "provider": {"path": format!("lib/onnxruntime/{provider}"), "bytes": provider_bytes, "blake3": provider_hash}, "model": {"path": "assets/models/beat-this/small0.onnx", "bytes": 10555597, "blake3": model_hash}, "scope": "Byte-validated resource preparation; not a native model Run or music quality result"});
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(package.join("NATIVE-ASSETS.json"))
        .map_err(|e| e.to_string())?;
    writeln!(output, "{receipt:#}").map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_guard_accepts_exact_bytes_and_rejects_size_hash_and_type() {
        let directory =
            std::env::temp_dir().join(format!("cocobeat-assets-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("empty");
        File::create(&path).unwrap();
        let empty_hash = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";
        assert!(checked(&path, 0, empty_hash).is_ok());
        assert!(checked(&path, 1, empty_hash).is_err());
        assert!(checked(&path, 0, &"0".repeat(64)).is_err());
        assert!(checked(&directory, 0, empty_hash).is_err());
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
