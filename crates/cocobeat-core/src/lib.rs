//! Pure gameplay rules, independent of rendering, hardware clocks and transport.
//!
//! Judge, one-to-one matching and DuoEngine arrive with the local vertical slice.
//! Their input is timestamped schema data; their output is semantic facts.
//! Replay and live input must use the same reducers. Presentation cannot mutate
//! those reducers. No placeholder judgement or hidden free-play score lives here.
