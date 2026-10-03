use cocobeat_schema::SongTime;
use cocobeat_stage::SegmentKind;
use std::path::Path;

pub fn inspect(path: &Path, frame: &str) -> Result<(), String> {
    let frame = frame
        .parse::<i64>()
        .map_err(|_| "Stage frame must be a signed integer".to_string())?;
    let package = cocobeat_media::validate_package(path)?;
    let hash: String = package
        .manifest
        .package_hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let plan = cocobeat_stage::compile(
        &format!("package-blake3:{hash}"),
        SongTime::from_frames(package.manifest.canonical_frames as i64),
        &package.analysis.sections,
    )?;
    let sample = plan
        .sample(SongTime::from_frames(frame))
        .ok_or_else(|| format!("Stage frame must be in 0..={}", plan.end().frames()))?;
    println!(
        "{}",
        serde_json::json!({
            "content_id": plan.content_id(),
            "compiler_version": plan.compiler_version(),
            "segment_count": plan.segments().len(),
            "end_frames": plan.end().frames(),
            "frame": frame,
            "kind": match sample.kind {
                SegmentKind::Straight => "straight",
                SegmentKind::Plaza => "plaza",
                SegmentKind::Curve => "curve",
                SegmentKind::Bridge => "bridge",
            },
            "distance_mm": sample.distance_mm,
            "half_width_mm": sample.half_width_mm,
            "lateral_mm": sample.lateral_mm,
            "elevation_mm": sample.elevation_mm,
            "slope_x_ppm": sample.slope_x_ppm,
            "slope_y_ppm": sample.slope_y_ppm,
            "at_end": frame == plan.end().frames(),
        })
    );
    Ok(())
}
