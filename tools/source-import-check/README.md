# Source import software check

This stdlib harness calls a fixed production lab executable and a separately fixed native game executable, each checked against its successful build receipt before every command; it creates a new owned evidence directory and preserves all failed logs

```sh
python tools/source-import-check/check.py --lab /path/to/fixed-lab --lab-receipt /path/to/lab-build-result.json --game /path/to/fixed-game --game-receipt /path/to/game-build-result.json --dev-source /path/to/dev-song-64s.wav --output /path/to/new-evidence
```

The build receipts require `status = PASS`, `exit_code = 0`, `inputs_unchanged = true`, `binary_sha256` and `scope`; the 64-second original source must match the documented `3390dd...` SHA256, and the short original stereo input is generated at 44.1 kHz with 4410 source frames

It checks production import, four-object integrity via `verify-package`, actual PCM N and finite values via strict `readback-canonical`, preserved source and object SHA256, recorded source BLAKE3 / rate / N / encoder profile, integer Stage sampling and EOF, and five rejected import controls; source BLAKE3 is recorded from validated analysis diagnostics, while independent BLAKE3 comparison belongs to the package module test

Two owned headless gamescope sessions execute production `--package ... --section-smoke ...`, compare the loaded content identity / N / complete integer Stage sample against lab output, and retain actual GPU PNG readbacks; PNG capture and matching state do not replace visual review, real audio, physical input, human listening or four-platform import acceptance
