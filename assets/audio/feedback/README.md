# Feedback palette

Nine selected VCSL recordings provide real soft, medium and hard attacks for claves, solo clap and glockenspiel, with authored noise, additive pluck and FM layers completing six timbre families

Source repository: https://github.com/sgossner/VCSL/tree/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e

VCSL recordings are CC0-1.0, with the complete upstream notice in LICENSE-VCSL.txt and exact upstream paths, source WAV SHA-256 hashes and shipped PCM SHA-256 hashes in SOURCES.json

Shipped files are headerless signed 16-bit little-endian mono PCM at 48 kHz, embedded by feedback_audio.rs; all nine together occupy 227,520 bytes and require no decoder or external tools at runtime

Each upstream WAV was converted with FFmpeg using `silenceremove=start_periods=1:start_threshold=-65dB:start_duration=0.0001,atrim=duration=D,afade=t=in:d=0.0003,afade=t=out:st=D-0.03:d=0.03`, resampled to 48 kHz and downmixed to mono, where D is 0.13 seconds for claves, 0.18 for clap and 0.48 for glockenspiel

A direct sinusoidal spectral scan of the selected glockenspiel recordings measures their dominant partial at 1053–1054 Hz, so the palette retunes that recorded partial to its common 523.2511 Hz bank root before chord-based transposition

Procedurally generated sound output is dedicated to CC0-1.0 by the CoCoBeat project; generator code remains under the repository MPL-2.0 license

The palette prepares three distinct recorded or synthesized velocity layers and four deterministic articulation variants per family, with separate dense and sparse tail lengths; variants are generated from the selected sources and are not represented as additional recorded round robins

Unknown harmony plays contact percussion only; trusted chord masks enable nearest-note melodic voices, and confirmed cooperation adds a complementary musical answer timed to the current beat subdivision

Each cached voice peak is at most 0.036, leaving room for Kira cubic interpolation and pan gain so each rendered voice remains below 0.055, and the runtime must enforce MAX_FEEDBACK_VOICES plus music headroom; independent track volume settings remain the caller’s responsibility
