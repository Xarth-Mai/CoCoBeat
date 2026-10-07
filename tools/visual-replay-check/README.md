# Native Replay watcher check

`check.py` uses only Python's standard library and the installed `gamescope` headless compositor; it launches the fixed production game through `--package DIR --watch-replay FILE`, with explicit native watcher observation enabled only for the owned QA processes

```bash
python3 tools/visual-replay-check/check.py \
  --game /absolute/fixed/bin/cocobeat-game \
  --receipt /absolute/game-build-result.json \
  --lab /absolute/fixed/bin/cocobeat-lab \
  --package /absolute/validated/package \
  --output /absolute/new-observation-directory
```

The receipt must record a successful build, unchanged source inputs and the exact game SHA-256; the script also hashes the independent lab binary and all four package objects

Use `--sizes small` to rerun only the two 640×480 partial cases after a presentation correction

The QA scope is a 2-to-100-second package, with Stage versions 1 and 2 at 1280×800 and 640×480; the full intro precedes fresh Ready confirmation, acknowledged playback, pause, resume, natural EOF, restart at the original epoch and return to the menu

Each case includes a valid 1,000-Hit prefix held behind an original future watermark; the watcher must retain the complete original fact order, coalesce simultaneous presentation sounds, match every original core event and keep partial histories partial

The wrapper records actual source cursor frames and Stage sampling at the acknowledged pause, plus separate same-integer-frame Stage reconstruction through the existing lab command; four native pauses are not claimed to occur at an identical audio callback frame

Original Replay/package bytes and binaries must remain unchanged, no `replays/` directory may appear, all owned screenshots must match the requested physical dimensions, and bounded CSV rows must retain original epoch and acknowledged cursor equality

Native window/GPU and callback observations are software evidence; physical controllers, speaker latency, human experience and historical network arrival timing require separate acceptance

Failed attempts remain in their own output directories; the script stops only its owned process group after its deadline
