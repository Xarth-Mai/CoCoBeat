# Native runtime performance observations

Opt-in Linux software measurements reuse the production native window, automatic Bevy Time, Kira output, compiled StagePlan, Session/core, feedback, quality and pacing; synthetic CapturedControl does not certify physical controls

`performance_probe::install_if_requested(&mut app)?` is called after Game, InputState, AudioOutput, brand and update_game installation

The module installs nothing unless COCOBEAT_PERFORMANCE_DIR is present; the runner supplies an isolated settings directory and a new output directory, applies explicit physical dimensions/scale/quality/pacing, and requires a compiled-stage package lasting at least 62 seconds

The probe caches First start-to-start monotonic Instant boundaries after production pacing and before TimeSystems; state labels and actual window/render texture dimensions are observed in PostUpdate; frames remain in RAM until completion or failure, bounded to 200000 rows and 150 wall-clock seconds

The probe origin begins after package decode, AudioOutput creation and base_app setup, so probe_install_seconds_to_stable_ready excludes earlier cold startup; the runner separately records process lifetime without equating it with render startup

Every process plays the complete production intro, then waits for brand completion, Ready, controls enabled and settled display; Ready has 5 seconds warmup and 10 seconds sampling; one contiguous actual Running segment has 10 seconds warmup and 50 seconds sampling; startup/transitions/warmup/EOF rows remain in raw evidence

The independent standard-library worker receives actual ClockBridge audio publications through a bounded channel, recalculates absolute deadlines using checked Instant arithmetic and upward nanosecond rounding, and captures its real waking Instant relative to input.origin; rendering later drains those original CapturedControl timestamps through the existing Session history mapping

Closing the sole publication sender interrupts the worker wait and joins it; future, invalid or older-than-250-ms audio publications stop capture, worker errors/panics and missing or unexpected inputs invalidate the run; publication and capture queues are bounded by one anchor and the finite schedule

The frozen schedule requests a free pair at 12 seconds, one local Hit at 18 seconds, two-player Good and Precise attempts on selected package Anchors and skipped Anchors for Miss; the actual captured times and original core results decide workload coverage, and absent expected grades invalidate the run rather than rewriting facts

Window requested AutoVsync/AutoNoVsync and normalized frame-limit settings are recorded and checked; resolved compositor/surface presentation mode, displayed frame count and monitor drops are not measured; main-update cadence is not GPU elapsed time

RSS includes the opt-in row cache and the production decoded source PCM; it is not an uninstrumented steady gameplay heap measurement

The runner copies the release binary into a new owned case directory, checks its SHA against a successful source-frozen Cargo release record, freezes package bytes, records native adapter/device features, collects its child PID /proc VmRSS/VmHWM at 5 Hz with process start identity, and retains stderr, package hashes, runtime Replay, original confirmed events, input captures and all failures

The recorded release command must contain --release and the build record must provide status PASS, inputs_unchanged true and binary_sha256; the caller supplies the complete source/dependency build evidence and excludes concurrent GPU, Cargo, encoder, MIR or other game workload during each case

```sh
python tools/runtime-performance-check/summarize.py
python tools/runtime-performance-check/run.py target/performance-20261007/new-case \
  --binary target/frozen-release/bin/cocobeat-game \
  --build-record target/frozen-release/build-result.json \
  --package target/source-import-20261007/cli-gpu-native/original-64s-package \
  --size 1920x1080 --quality medium --pacing unlimited
```

Run the 1280x800, 1920x1080 and 2560x1440 low/medium/high/off cases sequentially, plus medium 1920x1080 limited60 and vsync; retain every case; after the initial matrix repeat the measured highest-cost valid case twice in new directories

The standard-library summarizer computes nearest-rank p50/p90/p95/p99, mean/max, aggregate updates per elapsed interval, threshold counts, spike song-frame associations and longest slow streak; Ready/Running sample eligibility requires actual dimensions/scale, settled focus/display, matching quality/pacing and actual two-player FreeSync/AnchorSync/Precise/Good/Miss coverage plus sampled Curve, Bridge and authored SectionCue

Producer receipts retain nominal target frames, the original Instant converted again as moment_ns, actual capture timestamps, their audio anchor and scheduled deadline; the summarizer recomputes the signed ceiling deadline and rejects backdating, stale anchors, target/capture identity differences, worker failure or actual LateOrEarly grades

Gamescope 3.16.25 has a native Wayland borrowed-surface cleanup failure in its external WSI layer; an explicitly authorized local QA plan can set DISABLE_GAMESCOPE_WSI=1, which the runner records in every manifest, while preserving the default-layer crash evidence; successful wrapper exit is never a substitute for actual game child exit zero

The independent_instant_v2 producer changes the instrumentation identity; historical frame-polled 68a results remain separate and do not combine with a new frozen binary into a homogeneous matrix

These are descriptive local measurements with no universal hardware budget or formal product acceptance; GPU elapsed, draw calls, VRAM, audio underruns, physical input, speaker latency, four-platform native graphics, dual-machine and human acceptance remain NOT MEASURED
