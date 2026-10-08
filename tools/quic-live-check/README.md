# Production LiveSession loopback check

Use a completed root Cargo JSON build containing the exact `cocobeat-net`, media, schema, core, Replay and `serde_json` rlibs, and a validated package with at least 4800 frames

```sh
python tools/quic-live-check/check.py build target/quic-live-20261007/build.jsonl target/quic-live-20261007/probe-src/build
python tools/quic-live-check/check.py fixture target/quic-live-20261007/probe-src/build/driver testdata/synthetic/media-import/stereo-canonical.ogg target/quic-live-20261007/fixture
python tools/quic-live-check/check.py run target/quic-live-20261007/probe-src/build/driver target/quic-live-20261007/fixture/package target/quic-live-20261007/loopback
```

The public API driver uses two real production workers, explicit Ready, a future Scheduled deadline and explicit software Armed, then dynamically supplies different player histories and each player's final watermark before End

Fifteen bounded scenarios check installed and received packages, byte-preserved transfer, peer delivery, authority acknowledgment and same-core Replay equality, cancellation before Ready and after an accepted Hit, a missing Armed acknowledgment, wrong player or epoch input rejection, new-epoch reentry, and one-shot same-epoch recovery with rejected source identity, unauthenticated candidates, second loss, cancellation and End crossing the gate

Build metadata binds source files, exact rlibs and the driver; each run preserves commands, stdout, stderr, session evidence hashes and a summary without copying invitation secrets into the summary

Armed here acknowledges the software probe only; these checks provide no PCM scheduling, speaker timing, physical input, game window or two-machine acceptance evidence

The optional fixture command strictly decodes the original licensed synthetic 4800-frame Ogg, repeats its exact float PCM 20 times for a two-second WAV, invokes installed FFmpeg/libvorbis as an explicitly external QA encoder, and constructs the package with complete production readback and measured whole-file energy; `--repeat 12` yields 1.2 seconds, and any encoding or strict-readback error fails rather than changing the frame claim

The longer fixture provides native game probes enough time for Running and capture; its external encoder does not establish production encoder admission, MIR quality or music annotations

## Same-epoch recovery

The recovery software model explicitly advances an integer 48 kHz oscillator and submits source-publication intervals, real owner facts and watermarks through the public production worker API; it is not a Kira or CPAL observation

The separate `recovery-udp-blackhole` scenario uses a validated long package, such as the production 64-second original package. A bounded two-client UDP relay drops both directions until the first actual RecoveryPausing event, then restores forwarding and maps the continuation client's new source port. It records raw packet counters and actual production recovery causes without RequestRecovery, changing the invitation's endpoint only; byte reversal verifies the remaining invitation bytes

Protocol 7 records actual periodic ClockMaintained exchanges and keeps the original two-second freshness bound. For a typed maintenance-stale recovery, each triggering role must have a recorded pre-pause sample older than that bound, mapped from its actual host t2 / guest t4 through the Scheduled process origin into the relay coordinate. Both mappings, packet-drop evidence, original FIFO, snapshots and authority checks remain required. When the observed cause is instead the original reliable-input deadline, the existing 29-second blackhole minimum remains required; the earlier deadline observation is not a new run of that branch

The original deadline test observed reliable frame / peer-progress Deadline, not QUIC Connection TimedOut. The raw FAIL from the initial one-client relay, corrected mapping, historical scope / candidate-report fields and final equivalent removal of a redundant budget check remain separately recorded

```sh
python3 tools/quic-live-check/native.py FIXED_GAME ORIGINAL_64S_PACKAGE NEW_OUTPUT --compositor gamescope --scenario same-epoch-active --receipt GAME_BUILD_RECEIPT.json
```

This native scenario uses two actual game processes and the original Kira source, explicitly requests connection maintenance after both players' first two accepted Hits, and verifies the real pause / future resume / past-publication gate before a third performing Hit. During Recovering it queues one negative CapturedControl Hit through the real update loop, observes queue removal and unchanged local diagnostics / owner facts, and preserves the full GUI history prefix and worker snapshots. Each player must finish with exactly Hit seq 0, 1, 2; the blocked probe cannot appear in authority

The wrapper verifies the recorded game binary, compiler input receipt, actual child PID / exit, unchanged package objects and snapshots, authority bytes and core events. DISABLE_GAMESCOPE_WSI=1 avoids the separately retained local Gamescope WSI layer teardown crash; wrapper exit alone is not game exit evidence. Native connection maintenance and the UDP-deadline model are complementary tests, neither proves two-machine audio, physical controllers, DAC timing or long-term drift
