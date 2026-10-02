use serde_json::Value;
use std::{
    path::Path,
    process::{Command, ExitCode},
};

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    if let Err(error) = std::env::set_current_dir(root) {
        eprintln!("Cannot enter workspace: {error}");
        return ExitCode::FAILURE;
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command] if command == "doctor" => doctor(),
        [command] if command == "boundaries" => boundaries(),
        [command] if command == "check" => check(),
        [] => {
            println!("Usage: cargo xtask <doctor|boundaries|check>");
            Ok(())
        }
        _ => Err("Unknown arguments. Available commands: doctor, boundaries, check".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn cargo() -> std::ffi::OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

fn run(program: &std::ffi::OsStr, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program:?} {} failed: {status}", args.join(" ")))
    }
}

fn doctor() -> Result<(), String> {
    run(std::ffi::OsStr::new("rustc"), &["--version"])?;
    run(&cargo(), &["--version"])?;
    run(&cargo(), &["fmt", "--version"])?;
    run(&cargo(), &["clippy", "--version"])?;
    boundaries()?;
    println!("Day 0 toolchain and graph are ready. Audio/GPU/device timing is not checked.");
    Ok(())
}

fn check() -> Result<(), String> {
    boundaries()?;
    run(&cargo(), &["fmt", "--all", "--", "--check"])?;
    run(
        &cargo(),
        &[
            "clippy",
            "--locked",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run(&cargo(), &["test", "--locked", "--workspace"])
}

fn boundaries() -> Result<(), String> {
    let output = Command::new(cargo())
        .args(["metadata", "--locked", "--no-deps", "--format-version", "1"])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let metadata: Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    verify_graph(&metadata)?;
    println!("Workspace dependency boundaries OK (including dev/build/target dependencies).");
    Ok(())
}

/// Check canonical package names, not dependency aliases. Cargo metadata includes
/// declarations for every target and dependency kind, even inactive ones.
fn verify_graph(metadata: &Value) -> Result<(), String> {
    let packages = metadata["packages"].as_array().ok_or("Missing packages")?;
    let members = metadata["workspace_members"]
        .as_array()
        .ok_or("Missing workspace members")?;
    for package in packages.iter().filter(|p| members.contains(&p["id"])) {
        let name = package["name"].as_str().ok_or("Missing package name")?;
        let (allowed, allow_external): (&[&str], bool) = match name {
            "cocobeat-schema" => (&[], false),
            "cocobeat-core" => (&["cocobeat-schema"], false),
            "cocobeat-replay" => (
                &["cocobeat-schema", "cocobeat-core", "serde", "serde_json"],
                false,
            ),
            "cocobeat-runtime" => (
                &["cocobeat-schema", "cocobeat-core", "cocobeat-replay"],
                true,
            ),
            "cocobeat-game" => (&["cocobeat-runtime"], false),
            "cocobeat-lab" => (
                &[
                    "cocobeat-schema",
                    "cocobeat-core",
                    "cocobeat-replay",
                    "cocobeat-runtime",
                ],
                true,
            ),
            "xtask" => (&["serde_json"], false),
            _ => {
                return Err(format!(
                    "New workspace member {name}: define its dependency boundary first"
                ));
            }
        };
        for dependency in package["dependencies"]
            .as_array()
            .ok_or("Missing dependencies")?
        {
            let dependency_name = dependency["name"]
                .as_str()
                .ok_or("Missing dependency name")?;
            // Only registry/git dependencies are external. Unknown local helper
            // crates cannot be used to tunnel through a workspace boundary.
            let external =
                !dependency["source"].is_null() && !dependency_name.starts_with("cocobeat-");
            if !allowed.contains(&dependency_name) && !(allow_external && external) {
                return Err(format!("Forbidden dependency: {name} -> {dependency_name}"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn graph(name: &str, dependencies: Value) -> Value {
        json!({"workspace_members": ["local"], "packages": [
            {"id": "local", "name": name, "dependencies": dependencies}
        ]})
    }

    #[test]
    fn blocks_engine_even_as_renamed_target_dev_dependency() {
        let metadata = graph(
            "cocobeat-core",
            json!([
                {"name": "bevy", "rename": "innocent_name", "kind": "dev",
                 "target": "cfg(windows)", "source": "registry+https://example.invalid"}
            ]),
        );
        assert!(
            verify_graph(&metadata)
                .unwrap_err()
                .contains("cocobeat-core -> bevy")
        );
    }

    #[test]
    fn protects_direction_and_local_helper_boundary() {
        for (package, dependency) in [
            ("cocobeat-schema", "cocobeat-core"),
            ("cocobeat-core", "cocobeat-runtime"),
            ("cocobeat-game", "cocobeat-core"),
            ("cocobeat-runtime", "helper-with-hidden-engine"),
        ] {
            assert!(
                verify_graph(&graph(
                    package,
                    json!([{"name": dependency, "source": null}])
                ))
                .is_err()
            );
        }
    }

    #[test]
    fn accepts_owned_contracts_and_runtime_implementation_dependencies() {
        assert!(
            verify_graph(&graph(
                "cocobeat-core",
                json!([{"name": "cocobeat-schema", "source": null}])
            ))
            .is_ok()
        );
        assert!(
            verify_graph(&graph(
                "cocobeat-runtime",
                json!([{"name": "bevy", "source": "registry+https://example.invalid"}])
            ))
            .is_ok()
        );
        assert!(verify_graph(&graph("cocobeat-surprise", json!([]))).is_err());
    }
}
