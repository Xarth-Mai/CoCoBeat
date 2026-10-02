//! Capture and reproduce timestamped facts through the pure gameplay reducers.
//!
//! Day 0 establishes this boundary only. Recorder, persistent format and playback
//! are implemented alongside the first real input stream, never as a second rule
//! engine. Audio is not embedded in replay files; telemetry stays local by default.
