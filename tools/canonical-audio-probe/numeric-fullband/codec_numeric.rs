// Scratch-only arithmetic budget and observations; no sample values are changed
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

const EXPONENTS: [i32; 14] = [14, 39, 13, 0, 38, 39, 40, 85, 5, 47, 49, 86, 2, 87];
static MAXIMUM: [AtomicU32; 14] = [const { AtomicU32::new(0) }; 14];
static CALLS: [AtomicUsize; 14] = [const { AtomicUsize::new(0) }; 14];

pub fn observe(slot: usize, values: &[f32]) {
    let limit = 2.0_f32.powi(EXPONENTS[slot]);
    let mut peak = 0.0_f32;
    for value in values {
        assert!(value.is_finite(), "non-finite numeric stage {slot}");
        peak = peak.max(value.abs());
    }
    assert!(
        peak <= limit,
        "numeric stage {slot} exceeded conditional arithmetic budget"
    );
    MAXIMUM[slot].fetch_max(peak.to_bits(), Ordering::Relaxed);
    CALLS[slot].fetch_add(1, Ordering::Relaxed);
}

pub fn value(slot: usize, value: f32) {
    observe(slot, &[value]);
}

pub fn dump() {
    let maximum: Vec<_> = MAXIMUM
        .iter()
        .map(|value| f64::from(f32::from_bits(value.load(Ordering::Relaxed))))
        .collect();
    let calls: Vec<_> = CALLS
        .iter()
        .map(|value| value.load(Ordering::Relaxed))
        .collect();
    eprintln!(
        "NUMERIC {{\"maximum_abs\":{maximum:?},\"calls\":{calls:?},\"bound_exponents\":{EXPONENTS:?}}}"
    );
}

pub fn budget() -> String {
    assert!(crate::mdct::numeric_constants_valid());
    let tuning = crate::psy::tune();
    assert!(tuning.q_scale == 30.0 && tuning.ceil == 0.5 && tuning.nfloor == 1e-4);
    assert!(tuning.spread_up == -10.0 && tuning.spread_dn == -27.0);
    let tables = crate::setup::parse_setup(crate::setup::SETUP_Q4_STEREO, 2).unwrap();
    let mut codebook_max = 0.0_f32;
    let mut max_dimension = 0;
    let mut max_length = 0;
    for book in &tables.codebooks {
        max_dimension = max_dimension.max(book.dimensions);
        max_length = max_length.max(*book.lengths.iter().max().unwrap());
        if let Some(values) = &book.vq {
            assert!(values.iter().all(|value| value.is_finite()));
            codebook_max = values
                .iter()
                .map(|value| value.abs())
                .fold(codebook_max, f32::max);
        }
    }
    assert!(codebook_max <= 2047.0 && max_dimension <= 32 && max_length <= 32);
    let floor_min = crate::floor::FLOOR1_INV_DB
        .iter()
        .copied()
        .fold(f32::INFINITY, f32::min);
    assert!(floor_min > 2.0_f32.powi(-24));
    assert!(
        crate::floor::FLOOR1_INV_DB
            .iter()
            .all(|value| value.is_finite() && *value <= 1.0)
    );
    assert_eq!(tables.residues[1].partition_size, 32);
    assert_eq!(tables.residues[1].end, 2048);
    let mut work_bound = 2.0_f32.powi(39);
    for _ in 0..8 {
        work_bound = (work_bound + codebook_max).next_up();
    }
    let difference_bound = (work_bound + codebook_max).next_up();
    assert!(difference_bound < 2.0_f32.powi(40));
    format!(
        "{{\"status\":\"CONDITIONAL_ARITHMETIC_BUDGET_NOT_CODEC_ADMISSION\",\"pcm_limit\":4,\"mdct_limit\":16384,\"floor_min\":{floor_min},\"codebook_max_abs\":{codebook_max},\"max_book_dimension\":{max_dimension},\"max_codeword_length\":{max_length},\"cascade_work_bound\":{work_bound},\"vq_difference_bound\":{difference_bound},\"bound_exponents\":{EXPONENTS:?}}}"
    )
}
