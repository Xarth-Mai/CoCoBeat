// Appended only to a scratch copy of the frozen upstream resampler module
fn add_upper(a: f64, b: f64) -> f64 {
    (a + b).next_up()
}

fn product_upper(a: f64, b: f64) -> f64 {
    (a * b).next_up()
}

fn output_upper(coefficients: &[f64], peak: f64) -> f32 {
    let mut bound = 0.0;
    for coefficient in coefficients {
        bound = add_upper(bound, product_upper(peak, coefficient.abs()));
    }
    let result = bound as f32;
    if f64::from(result) < bound {
        result.next_up()
    } else {
        result
    }
}

pub fn numeric_certificate(rate: u32, output: &std::path::Path) -> String {
    let mut engine =
        SincEngine::new(rate, 48_000, 2, ResamplerQuality::High.sinc_params()).unwrap();
    assert_eq!(engine.taps, 192);
    assert_eq!(engine.oversampling, 256);
    assert!(engine.table.iter().all(|value| value.is_finite()));
    let bytes: Vec<_> = engine
        .table
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    std::fs::write(output, bytes).unwrap();
    let row_bound = engine
        .table
        .chunks(engine.taps)
        .map(|row| {
            row.iter()
                .fold(0.0, |bound, value| add_upper(bound, value.abs()))
        })
        .fold(0.0_f64, f64::max);
    let (mut actual_l1, mut unit_bound, mut fixture_bound) = (0.0_f64, 0.0_f32, 0.0_f32);
    for phase in 0..engine.denom {
        engine.frac_num = phase;
        engine.fill_scratch();
        assert!(engine.scratch.iter().all(|value| value.is_finite()));
        actual_l1 = actual_l1.max(
            engine
                .scratch
                .iter()
                .fold(0.0, |bound, value| add_upper(bound, value.abs())),
        );
        unit_bound = unit_bound.max(output_upper(&engine.scratch, 1.0));
        fixture_bound = fixture_bound.max(output_upper(&engine.scratch, 32700.0 / 32768.0));
    }
    assert!(fixture_bound.is_finite() && unit_bound < 3.0);
    let unit_bound = f64::from(unit_bound);
    let fixture_bound = f64::from(fixture_bound);
    format!(
        "{{\"source_rate\":{rate},\"phases\":{},\"stored_row_real_l1_upper\":{row_bound},\"actual_interpolated_l1_upper\":{actual_l1},\"output_f32_upper_for_peak1\":{unit_bound},\"output_f32_upper_for_fixture_peak\":{fixture_bound}}}",
        engine.denom
    )
}
