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
