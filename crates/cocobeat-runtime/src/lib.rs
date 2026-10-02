//! Implementation boundary for Bevy, Kira, input capture and presentation.
//!
//! External engine/audio adapters are added when the timing lab needs them.
//! FeedbackDirector will consume semantic events, never write back into Judge.

/// Day 0 executable entry point. This is a bootstrap diagnostic, not gameplay.
pub fn run() {
    println!("CoCoBeat · Day 0 workspace");
    println!(
        "Canonical timeline: {} Hz audio frames",
        cocobeat_schema::CANONICAL_SAMPLE_RATE
    );
    println!("The playable 64-second slice is not implemented yet. See todo/README.md.");
}
