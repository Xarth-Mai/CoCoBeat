#!/usr/bin/env python3
"""QA preparation only: freeze native sources and reuse old Rust generators/matcher"""
import hashlib
import json
from pathlib import Path
import shutil
import sys

ROOT = Path(__file__).resolve().parents[2]
AUDIOFLUX = ROOT / "target/mir-permissive-20261007/audioflux-source"
DECLARATION = "testdata/synthetic/mir-flux-gate-probe/declared-band-background-20261003.json"
DECLARATION_SHA = "605f83ed62e7a9eb14e3d2905f7948bfe0dbb5b21f7d9bbf85c7a034a632ee4c"
OLD = {
    "onset": "tools/mir-onset-probe/src/main.rs",
    "timing": "tools/mir-onset-diagnostic/src/main.rs",
    "flux": "tools/mir-flux-probe/src/main.rs",
    "gate": "tools/mir-flux-gate-probe/src/main.rs",
}
OLD_SHA = {
    "onset": "cda827107b9e5ada153ad9d57fe10526da251a54521e074cc13db0507ce01a26",
    "timing": "4a0d7d4782f8a1a97aa77071559f42be2519c29f952d7e96219ccfd74c6743c9",
    "flux": "5343abafc6379100e6766b8d7fc6ea075803f88823a4eb4a149aeac84b8cd6f8",
    "gate": "434d3708ede7f9201a92dac408a6b4eca636538631f5a8544d3067b674387756",
}
CONTROLS = "testdata/synthetic/mir-flux-probe/declared-controls-20261003.json"
NEXT = "testdata/synthetic/mir-flux-gate-probe/declared-next-controls-20261003.json"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def prepare(output):
    manifest = json.loads((AUDIOFLUX / "manifest.json").read_text())
    assert manifest["git_commit"] == "0c3f55b409b07381bfe770711e3642e19f333bee"
    assert manifest["commit_tree_files_match"]
    assert sha((ROOT / DECLARATION).read_bytes()) == DECLARATION_SHA
    for key, relative in OLD.items():
        assert sha((ROOT / relative).read_bytes()) == OLD_SHA[key], relative
    for item in manifest["files"]:
        data = (AUDIOFLUX / item["path"]).read_bytes()
        assert sha(data) == item["sha256"] and len(data) == item["bytes"]
    output.mkdir()
    shutil.copytree(AUDIOFLUX, output / "audioflux")
    paths = list(OLD.values()) + [DECLARATION, CONTROLS, NEXT]
    paths += ["tools/native-onset-check/" + name for name in ["driver.c", "build.sh", "prepare.py", "check.py", "README.md"]]
    for relative in paths:
        destination = output / "frozen" / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / relative, destination)
    sources = {key: (output / "frozen" / path).read_text() for key, path in OLD.items()}
    fragments = []

    def cut(key, start, end=None):
        source = sources[key]
        begin = source.index(start)
        stop = source.index(end, begin) if end else len(source)
        text = source[begin:stop]
        fragments.append({"source": OLD[key], "start_line": source[:begin].count("\n") + 1,
                          "end_line": source[:stop].count("\n"), "sha256": sha(text.encode())})
        return text

    harness = "use serde_json::{Value, json};\nuse std::{fs, path::Path, io::Write};\n"
    harness += cut("onset", "const RATE:", "const WINDOW:")
    harness += cut("onset", "const TOLERANCE:", "fn predictions(")
    harness += "\nfn create(path: &Path, bytes: &[u8]) {\n"
    harness += "    fs::OpenOptions::new().write(true).create_new(true).open(path).unwrap().write_all(bytes).unwrap();\n}\n"
    harness += "fn pcm(root: &Path, name: &str, samples: &[f32]) {\n"
    harness += "    create(&root.join(format!(\"{name}.f32le\")), &samples.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>());\n}\n"
    harness += "fn regenerate(root: &Path) {\n    fs::create_dir(root).unwrap();\n"
    harness += cut("onset", "    let cases = [", "    let detector =")
    harness += "    for (name, frames, events, _truth, polarity) in cases {\n"
    harness += "        let samples: Vec<f32> = signal(frames, &events, polarity).into_iter().flatten().collect();\n"
    harness += "        pcm(root, name, &samples);\n    }\n"
    harness += cut("timing", "    let mut holdout =", "    let mut holdout_results =")
    harness += "    for (name, samples, _truth) in holdout { pcm(root, name, &samples); }\n"
    harness += "    flux::write(root);\n    gate::write(root);\n}\n"
    for key, declaration, array in [("flux", CONTROLS, "new_controls"), ("gate", NEXT, "controls")]:
        harness += f"mod {key} {{\n    use super::*;\n"
        if key == "flux":
            harness += cut(key, "fn noise(", "fn repeated_pick_control(")
        else:
            harness += cut(key, "fn generate(", "fn run_controls(")
        path = str((output / "frozen" / declaration).resolve())
        harness += "    pub fn write(root: &Path) {\n"
        harness += f"        let declaration: Value = serde_json::from_str(include_str!({json.dumps(path)})).unwrap();\n"
        harness += f'        for control in declaration["{array}"].as_array().unwrap() {{\n'
        harness += '            pcm(root, control["name"].as_str().unwrap(), &generate(control));\n'
        harness += "        }\n    }\n}\n"
    harness += r'''
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, output] if command == "regenerate" => regenerate(Path::new(output)),
        [command, declaration, index, prediction, output] if command == "score" => {
            let declared: Value = serde_json::from_slice(&fs::read(declaration).unwrap()).unwrap();
            let input = &declared["inputs"][index.parse::<usize>().unwrap()];
            assert!(input.is_object());
            let prediction: Value = serde_json::from_slice(&fs::read(prediction).unwrap()).unwrap();
            let predicted: Vec<usize> = serde_json::from_value(prediction["predicted_frames"].clone()).unwrap();
            assert!(predicted.windows(2).all(|v| v[0] < v[1]));
            assert!(predicted.iter().all(|&v| v < input["sample_frames"].as_u64().unwrap() as usize));
            let truth: Option<Vec<usize>> = serde_json::from_value(input["truth_frames"].clone()).unwrap();
            let score = truth.as_ref().map_or(Value::Null, |truth| metrics(truth, &predicted));
            create(Path::new(output), serde_json::to_string_pretty(&score).unwrap().as_bytes());
        }
        _ => panic!("usage: fixture-matcher regenerate NEW_ROOT | score DECLARATION INDEX PREDICTION NEW_SCORE"),
    }
}
'''
    harness += cut("onset", "#[cfg(test)]")
    (output / "fixture-matcher.rs").write_text(harness)
    declared = json.loads((ROOT / DECLARATION).read_text())
    inputs = []
    for item in declared["inputs"]:
        name = item["case"].split("/channel-")[0]
        inputs.append({**item, "regenerated_filename": f"{name}.f32le",
                       "original_present": (ROOT / item["pcm_path"]).is_file()})
    recipe = {"status": "PREPARED_NOT_RUN", "audioflux_commit": manifest["git_commit"],
              "declaration_sha256": DECLARATION_SHA, "fragments": fragments,
              "original_source_sha256": {OLD[key]: value for key, value in OLD_SHA.items()},
              "inputs": inputs, "canonical_old_onset_matrix": "NOT_RUN: no matching encoded old34 inputs",
              "matcher": declared["matcher"], "production_admission": False}
    (output / "recipe.json").write_text(json.dumps(recipe, ensure_ascii=False, indent=2) + "\n")
    files = sorted(p for p in output.rglob("*") if p.is_file())
    (output / "source-files.sha256").write_text("".join(f"{sha(p.read_bytes())}  {p.relative_to(output)}\n" for p in files))
    print(output)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: prepare.py NEW_PREPARED_DIRECTORY")
    prepare(Path(sys.argv[1]).resolve())
