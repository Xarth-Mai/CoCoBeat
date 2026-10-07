"""One owned Linux native performance case; run cases sequentially with a frozen release binary."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import time

sys.dont_write_bytecode = True

from summarize import summarize


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write(path, value):
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, indent=2, allow_nan=False)
        stream.write("\n")


def package_inputs(path):
    if path.is_symlink() or not path.is_dir():
        raise ValueError("package must be a real directory")
    result = {}
    for item in sorted(path.rglob("*")):
        if item.is_symlink():
            raise ValueError(f"package symlink rejected: {item.name}")
        if item.is_file(): result[str(item.relative_to(path))] = sha(item)
    if not result: raise ValueError("empty package")
    return result


def memory_sample(pid, identity):
    path = Path(f"/proc/{pid}")
    try:
        stat = (path / "stat").read_text()
        start = stat[stat.rindex(")") + 2:].split()[19]
        if identity is not None and start != identity:
            raise ValueError("owned PID start identity changed")
        status = (path / "status").read_text().splitlines()
        values = {line.split(":", 1)[0]: line.split(":", 1)[1].strip() for line in status if line.startswith(("VmRSS:", "VmHWM:"))}
        return start, {"parent_monotonic_ns": time.monotonic_ns(), "pid": pid, "process_start_ticks": start, **values}
    except FileNotFoundError:
        return identity, None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("new_output", type=Path)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--build-record", type=Path, required=True)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--size", choices=("1280x800", "1920x1080", "2560x1440"), required=True)
    parser.add_argument("--quality", choices=("low", "medium", "high", "off"), required=True)
    parser.add_argument("--pacing", choices=("unlimited", "limited60", "vsync"), default="unlimited")
    args = parser.parse_args()
    if platform.system() != "Linux": parser.error("this owned-PID RSS runner currently supports Linux only")
    package = args.package.resolve()
    if args.package.is_symlink(): parser.error("package symlink rejected")
    before = package_inputs(package)
    build = json.loads(args.build_record.read_text())
    if build.get("status") != "PASS" or build.get("inputs_unchanged") is not True or "--release" not in build.get("command", []):
        parser.error("requires a successful frozen source release build record")
    binary_hash = sha(args.binary)
    if binary_hash != build.get("binary_sha256"): parser.error("binary SHA differs from release build record")
    output = args.new_output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    binary = output / "cocobeat-game"
    shutil.copy2(args.binary, binary)
    if sha(binary) != binary_hash: raise RuntimeError("copied binary identity changed")
    for name in ("config", "data"):
        (output / name).mkdir()
    env = {**os.environ, "XDG_CONFIG_HOME": str(output / "config"), "XDG_DATA_HOME": str(output / "data"), "COCOBEAT_PERFORMANCE_DIR": str(output / "probe"), "COCOBEAT_PERFORMANCE_SIZE": args.size, "COCOBEAT_PERFORMANCE_QUALITY": args.quality, "COCOBEAT_PERFORMANCE_PACING": args.pacing}
    for name in tuple(env):
        if name.startswith(("COCOBEAT_LIVE_OBSERVATION", "COCOBEAT_WATCH_OBSERVATION")): del env[name]
    command = [str(binary), "--package", str(package)]
    manifest = {"command": command, "binary_sha256": binary_hash, "build_record_sha256": sha(args.build_record), "package": str(package), "package_inputs_before": before, "host": {"platform": platform.platform(), "machine": platform.machine(), "python": platform.python_version(), "cpu_model": next((line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name")), None)}, "environment": {name: env[name] for name in ("DISPLAY", "WAYLAND_DISPLAY", "XDG_SESSION_TYPE", "WGPU_BACKEND", "DISABLE_GAMESCOPE_WSI", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "COCOBEAT_PERFORMANCE_DIR", "COCOBEAT_PERFORMANCE_SIZE", "COCOBEAT_PERFORMANCE_QUALITY", "COCOBEAT_PERFORMANCE_PACING") if name in env}, "memory_sampling_seconds": 0.2, "concurrency_policy": "one native game; caller must exclude other GPU/Cargo/encoder/MIR workload", "deadline_seconds": 180, "script_inputs": {path.name: sha(path) for path in Path(__file__).parent.glob("*.py")}, "scope": "one Linux native release case; /proc RSS observations exclude VRAM and do not measure speaker latency"}
    write(output / "manifest.json", manifest)
    samples = []
    identity = None
    failure = None
    with (output / "stdout.log").open("xb") as stdout, (output / "stderr.log").open("xb") as stderr:
        launched_ns = time.monotonic_ns()
        child = subprocess.Popen(command, cwd=output, env=env, stdout=stdout, stderr=stderr)
        deadline = time.monotonic() + 180
        try:
            while child.poll() is None:
                identity, sample = memory_sample(child.pid, identity)
                if sample is not None: samples.append(sample)
                if time.monotonic() >= deadline:
                    failure = "owned child exceeded 180-second deadline"
                    child.terminate()
                    try: child.wait(timeout=5)
                    except subprocess.TimeoutExpired: child.kill(); child.wait()
                    break
                time.sleep(0.2)
        except BaseException:
            child.terminate()
            try: child.wait(timeout=5)
            except subprocess.TimeoutExpired: child.kill(); child.wait()
            raise
    write(output / "memory.json", {"samples": samples, "scope": "owned child /proc VmRSS/VmHWM at 5 Hz; peak between samples may be missed; not steady heap or VRAM", "observed_peak_rss_kib": max((int(row["VmRSS"].split()[0]) for row in samples if "VmRSS" in row), default=None), "observed_peak_hwm_kib": max((int(row["VmHWM"].split()[0]) for row in samples if "VmHWM" in row), default=None)})
    exited_ns = time.monotonic_ns()
    after = package_inputs(package)
    script_unchanged = {path.name: sha(path) for path in Path(__file__).parent.glob("*.py")} == manifest["script_inputs"]
    build_record_unchanged = sha(args.build_record) == manifest["build_record_sha256"]
    final = {"parent_launch_monotonic_ns": launched_ns, "parent_observed_exit_monotonic_ns": exited_ns, "whole_child_lifetime_seconds": (exited_ns - launched_ns) / 1e9, "scripts_unchanged": script_unchanged, "build_record_unchanged": build_record_unchanged, "process_id": child.pid, "process_start_ticks": identity, "exit_code": child.returncode, "failure": failure, "package_inputs_after": after, "package_unchanged": before == after, "binary_unchanged": sha(binary) == binary_hash and sha(args.binary) == binary_hash, "artifacts": {str(path.relative_to(output)): sha(path) for path in sorted(output.rglob("*")) if path.is_file() and path != binary}}
    try:
        probe = output / "probe"
        result = summarize([json.loads(line) for line in (probe / "frames.jsonl").read_text().splitlines()], json.loads((probe / "metadata.json").read_text()), json.loads((probe / "result.json").read_text()))
        if child.returncode or failure or before != after or not final["binary_unchanged"] or not script_unchanged or not build_record_unchanged:
            result["status"] = "INVALID_LOCAL_OBSERVATION"
            result["invalid_reasons"].append("process/build/package identity or completion failure")
        write(output / "summary.json", result)
        final["status"] = result["status"]
    except (OSError, ValueError, KeyError, TypeError) as error:
        final["status"] = "INVALID_LOCAL_OBSERVATION"
        final["summary_error"] = str(error)
    write(output / "validation.json", final)
    print(json.dumps({"status": final["status"], "exit_code": child.returncode, "output": str(output)}))
    raise SystemExit(0 if final["status"] == "VALID_LOCAL_OBSERVATION" else 1)


if __name__ == "__main__":
    main()
