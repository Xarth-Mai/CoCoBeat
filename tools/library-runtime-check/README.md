# Native authored library check

This standard-library wrapper is prepared for the explicit production library observation hook; actual execution requires a fixed game binary, its successful unchanged-input build receipt, two different validated authored packages and a third structurally valid package with unsupported rules

```bash
python3 tools/library-runtime-check/check.py \
  --game /absolute/fixed/bin/cocobeat-game \
  --receipt /absolute/game-build-result.json \
  --lab /absolute/fixed/bin/cocobeat-lab \
  --lab-receipt /absolute/lab-build-result.json \
  --package-a /absolute/authored-a \
  --package-b /absolute/authored-b \
  --unknown-rules /absolute/unsupported-rules \
  --output /absolute/new-observation-directory
```

The wrapper copies packages into its own library root, creates separate truncated-audio and extra-file controls, hashes source objects, copied objects, the fixed binary, receipt and harness, and stops only its owned process group after the deadline

The local native condition disables the installed Gamescope WSI layer through its documented `DISABLE_GAMESCOPE_WSI=1` switch and removes only its known `ENABLE_GAMESCOPE_WSI` enable variable; it leaves other system and Vulkan settings intact, records the actual child's display / configuration / WSI environment, and rejects any child WSI initialization log

The same `check.py --child` supervisor runs the game inside the compositor, preserves the real game PID and signed subprocess return code with separate game stdout / stderr, writes the create-new `child-exit.json`, and requires a complete zero game exit in addition to the compositor wrapper's exit; timeout, missing child exit record, crash and nonzero exit retain raw evidence and reject PASS

Four complete cases cover 1280×800 and 640×480 in zh-CN and en-US; the production observer must drive the existing keyboard capture and actual menu, await the full background package loader, inspect Ready before issuing a separate Start, switch two packages, reject bad audio / extra files / unsupported rules, discard a cancelled result and close only after its owned worker exits

Two separate close cases request closure when scan or load is owned; they record whether work was actually unfinished at that instant instead of treating a completed worker as an in-flight cancellation test

Screenshots require separate visual review; synthetic controls, native GPU rendering and an acknowledged Kira cursor do not establish physical dual-controller / mixed-device behavior, speaker timing or human acceptance

The explicit observer interface is documented in the preparation plan under target/library-runtime-20261007/PLAN.md; do not run this draft against a binary without that matching hook

Callback and source publications are two independent historical software snapshots, with publication intervals and ages measured after reading; they are not a paired callback, CPAL-entry time or device latency. Ready expects no source publication; source and callback telemetry can change between rejection snapshots, so unchanged-song checks compare all original song/history fields while keeping the new telemetry as raw evidence

Complete cases also use actual ArrowDown presses to focus Refresh, Back and the first Information row, capture each settled focus, then return and reopen the library before loading; these captures distinguish reachable scrolling from rows that cannot all fit on screen, without activating Information or changing game state directly

## Ready source import

`source.py` reuses the same fixed-binary receipt validation and `check.py --child` supervisor for the normal no-argument game entry; a matching game build must include the Ready paired-source import UI and the seven `source-*` observer scenarios

```bash
python3 tools/library-runtime-check/source.py \
  --game /absolute/fixed/bin/cocobeat-game \
  --receipt /absolute/game-build-result.json \
  --lab /absolute/fixed/bin/cocobeat-lab \
  --lab-receipt /absolute/lab-build-result.json \
  --source /absolute/fixed/cocobeat-64.wav \
  --authoring assets/dev/vertical_slice/authoring.json \
  --output /absolute/new-source-observation-directory
```

The QA fixture is deliberately fixed to the original 64-second source (SHA-256 `3390dd080cb536fd4a598ea933618dd99b4bf697c0b2874ff5cb0e220feb09e8`) and its unchanged hand-authored document (`7eb6b8ad5971367e37099dec41b9729c98523895fe70f73df4a61a618281637e`); the game UI accepts supported paired sources independently of this test fixture

The helper creates owned absolute XDG_DATA_HOME / configuration roots and `songs/imports/00-source.wav` + `00-source.authoring.json` copies, launches the actual game with no CLI arguments, and uses only original KeyboardInput / WindowFocused / WindowCloseRequested capture and the full brand intro; the normal development song remains the baseline until the actual source importer and package loader succeed

Seven cases run by default; repeat `--scenario` to select an explicit subset

- `source-complete`: paired imports → source confirmation with a not-yet-created destination → actual worker / four objects / loader → Ready without automatic playback → fresh menu confirmation → original Kira cursor → two original captured Hits / Pause / saved Replay → normal close
- `source-back`: return while the source worker is owned, ordinary Ready confirmation, actual library reopen and preserved original song/history after completion
- `source-focus`: lose focus while the source worker is owned, restore focus and reopen without selecting its abandoned result
- `source-bad-json`: reject the actual malformed authoring document, preserve the original song and importer error, publish no destination
- `source-unknown-rules`: publish and structurally verify all four objects, then preserve the original song and actual runtime unsupported-rules rejection
- `source-background-error`: return during real encoding with an unknown-rules document, preserve the original loader error after publication and before an explicit refresh
- `source-close`: request normal closure with an owned worker, observe its real completion state and require the original join path plus separate zero game / compositor exits

Ready confirmation during unfinished import, reopening while still encoding and closing while still encoding each require actual importing / is_finished observations; a missed control window is separately NOT RUN, without artificial delays or direct game/session/song mutation. Background return abandons automatic selection while the actual importer continues; it does not claim cooperative cancellation or rollback

The summary preserves the actual command / PID / environment / signed exit, confirmed destination, object hashes and game importer profile, original lab verification / finite canonical readback, nonempty full-intro Ready frames, original capture timestamps and Replay, native screenshot callbacks / PNG dimensions and distinct NOT RUN subchecks. Failures preserve raw evidence and stop subsequent cases; output paths must be new

The formal helper differs from the frozen target execution runner only in repository-root lookup for its permanent path. AST / --help checks for this copy establish its structure and CLI; previous native evidence belongs to the recorded target runner SHA and is not a claim that the formal file itself was executed natively

PNG contents still require visual review; physical keyboard / dual gamepad / mixed input, mother-tongue and human acceptance, speaker / DAC timing, release performance and four-platform native source UI remain separate acceptance work. This hand-authored import path does not admit automatic MIR quality, invent Anchors or use empty Anchors as a fallback for failed analysis
