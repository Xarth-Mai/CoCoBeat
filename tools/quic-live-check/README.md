# Production LiveSession loopback check

Use a completed root Cargo JSON build containing the exact `cocobeat-net`, media, schema, core, Replay and `serde_json` rlibs, and a validated package with at least 4800 frames

```sh
python tools/quic-live-check/check.py build target/quic-live-20261007/build.jsonl target/quic-live-20261007/probe-src/build
python tools/quic-live-check/check.py fixture target/quic-live-20261007/probe-src/build/driver testdata/synthetic/media-import/stereo-canonical.ogg target/quic-live-20261007/fixture
python tools/quic-live-check/check.py run target/quic-live-20261007/probe-src/build/driver target/quic-live-20261007/fixture/package target/quic-live-20261007/loopback
```

The public API driver uses two real production workers, explicit Ready, a future Scheduled deadline and explicit software Armed, then dynamically supplies different player histories and each player's final watermark before End

Seven bounded scenarios check installed and received packages, byte-preserved transfer, peer delivery, authority acknowledgment and same-core Replay equality, cancellation before Ready and after an accepted Hit, a missing Armed acknowledgment, and wrong player or epoch input rejection

Build metadata binds source files, exact rlibs and the driver; each run preserves commands, stdout, stderr, session evidence hashes and a summary without copying invitation secrets into the summary

Armed here acknowledges the software probe only; these checks provide no PCM scheduling, speaker timing, physical input, game window or two-machine acceptance evidence

The optional fixture command strictly decodes the original licensed synthetic 4800-frame Ogg, repeats its exact float PCM 20 times for a two-second WAV, invokes installed FFmpeg/libvorbis as an explicitly external QA encoder, and constructs the package with complete production readback and measured whole-file energy; `--repeat 12` yields 1.2 seconds, and any encoding or strict-readback error fails rather than changing the frame claim

The longer fixture provides native game probes enough time for Running and capture; its external encoder does not establish production encoder admission, MIR quality or music annotations
