//! Deterministic song-time geometry, independent of rendering and music judgement

use cocobeat_schema::{
    CANONICAL_SAMPLE_RATE, MAX_CANONICAL_FRAMES, MAX_CONTENT_ITEMS, MAX_CONTENT_TEXT_BYTES,
    SectionFeature, SongTime,
};

pub use cocobeat_schema::STAGE_COMPILER_VERSION as COMPILER_VERSION;
pub const BASE_HALF_WIDTH_MM: i64 = 3_500;
const MAX_PLAZA_EXPANSION_MM: i64 = 500;
const FRAMES_PER_MM: i64 = 16;
const LONG_SECTION_FRAMES: i64 = 16 * CANONICAL_SAMPLE_RATE as i64;
const CURVE_OFFSET_MM: i64 = 600;
const BRIDGE_HEIGHT_MM: i64 = 300;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagePlan {
    content_id: String,
    compiler_version: u32,
    end: SongTime,
    segments: Vec<TrackSegment>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrackSegment {
    pub start: SongTime,
    pub end: SongTime,
    pub kind: SegmentKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentKind {
    Straight,
    Plaza,
    Curve,
    Bridge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrackSample {
    pub distance_mm: i64,
    pub half_width_mm: i64,
    pub lateral_mm: i64,
    pub elevation_mm: i64,
    pub slope_x_ppm: i64,
    pub slope_y_ppm: i64,
    pub kind: SegmentKind,
}

/// Compile real analysis intervals without inferring intervals from chart cues
pub fn compile(
    content_id: &str,
    end: SongTime,
    sections: &[SectionFeature],
) -> Result<StagePlan, String> {
    compile_version(content_id, end, sections, COMPILER_VERSION)
}

/// Version 1 keeps every analysis interval as a Plaza; version 2 adds Curve and Bridge
pub fn compile_version(
    content_id: &str,
    end: SongTime,
    sections: &[SectionFeature],
    compiler_version: u32,
) -> Result<StagePlan, String> {
    if !matches!(compiler_version, 1 | 2) {
        return Err(format!(
            "Unsupported stage compiler version: {compiler_version}"
        ));
    }
    if content_id.is_empty() || content_id.len() > MAX_CONTENT_TEXT_BYTES {
        return Err("Stage content identity must contain 1 to 256 UTF-8 bytes".into());
    }
    if !(1..=MAX_CANONICAL_FRAMES as i64).contains(&end.frames()) {
        return Err("Stage must cover more than zero and at most ten minutes".into());
    }
    if sections.len() > MAX_CONTENT_ITEMS {
        return Err("Stage section count exceeds the content item limit".into());
    }

    let max_long_sections = if compiler_version == 2 {
        sections
            .len()
            .min((end.frames() / LONG_SECTION_FRAMES) as usize)
    } else {
        0
    };
    let mut segments = Vec::with_capacity(sections.len() * 2 + 1 + max_long_sections);
    let mut through = SongTime::ZERO;
    for section in sections {
        if section.start < through || section.end <= section.start || section.end > end {
            return Err(
                "Stage sections must be nonempty, ordered and non-overlapping intervals within the song"
                    .into(),
            );
        }
        if section.start > through {
            segments.push(TrackSegment {
                start: through,
                end: section.start,
                kind: SegmentKind::Straight,
            });
        }
        let duration = section.end.frames() - section.start.frames();
        if compiler_version == 2 && duration >= LONG_SECTION_FRAMES {
            let middle = SongTime::from_frames(section.start.frames() + duration / 2);
            segments.push(TrackSegment {
                start: section.start,
                end: middle,
                kind: SegmentKind::Curve,
            });
            segments.push(TrackSegment {
                start: middle,
                end: section.end,
                kind: SegmentKind::Bridge,
            });
        } else {
            segments.push(TrackSegment {
                start: section.start,
                end: section.end,
                kind: SegmentKind::Plaza,
            });
        }
        through = section.end;
    }
    if through < end {
        segments.push(TrackSegment {
            start: through,
            end,
            kind: SegmentKind::Straight,
        });
    }
    Ok(StagePlan {
        content_id: content_id.into(),
        compiler_version,
        end,
        segments,
    })
}

impl StagePlan {
    pub fn content_id(&self) -> &str {
        &self.content_id
    }

    pub fn end(&self) -> SongTime {
        self.end
    }

    pub fn compiler_version(&self) -> u32 {
        self.compiler_version
    }

    pub fn segments(&self) -> &[TrackSegment] {
        &self.segments
    }

    /// Sample closed song bounds; the end retains the last kind at base width and zero offset
    /// Forward-axis distance and plaza width round down; offsets and slopes round to nearest,
    /// with exact halves away from zero
    pub fn sample(&self, time: SongTime) -> Option<TrackSample> {
        if time < SongTime::ZERO || time > self.end {
            return None;
        }
        let index = self.segments.partition_point(|segment| segment.end <= time);
        let segment = &self.segments[index.min(self.segments.len() - 1)];
        let duration = segment.end.frames() - segment.start.frames();
        let elapsed = time.frames() - segment.start.frames();
        let mut sample = TrackSample {
            distance_mm: time.frames() / FRAMES_PER_MM,
            half_width_mm: BASE_HALF_WIDTH_MM,
            lateral_mm: 0,
            elevation_mm: 0,
            slope_x_ppm: 0,
            slope_y_ppm: 0,
            kind: segment.kind,
        };
        match segment.kind {
            SegmentKind::Straight => {}
            SegmentKind::Plaza => {
                // Limit short sections to one millimetre of width per millimetre of travel
                let peak = MAX_PLAZA_EXPANSION_MM.min(duration / (2 * FRAMES_PER_MM));
                sample.half_width_mm += 2 * peak * elapsed.min(duration - elapsed) / duration;
            }
            SegmentKind::Curve => {
                (sample.lateral_mm, sample.slope_x_ppm) = bump(elapsed, duration, CURVE_OFFSET_MM);
            }
            SegmentKind::Bridge => {
                (sample.elevation_mm, sample.slope_y_ppm) =
                    bump(elapsed, duration, BRIDGE_HEIGHT_MM);
            }
        }
        Some(sample)
    }
}

// The quartic and its derivative vanish at both ends, joining each centreline without a kink
fn bump(elapsed: i64, duration: i64, amplitude: i64) -> (i64, i64) {
    let n = i128::from(elapsed);
    let d = i128::from(duration);
    let a = i128::from(amplitude);
    let denominator = d.pow(4);
    (
        round_ratio(16 * a * n.pow(2) * (d - n).pow(2), denominator),
        round_ratio(512 * a * n * (d - n) * (d - 2 * n) * 1_000_000, denominator),
    )
}

fn round_ratio(numerator: i128, denominator: i128) -> i64 {
    ((numerator + numerator.signum() * (denominator / 2)) / denominator) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(start: i64, end: i64) -> SectionFeature {
        SectionFeature {
            start: SongTime::from_frames(start),
            end: SongTime::from_frames(end),
            confidence: None,
            label: "authored".into(),
        }
    }

    #[test]
    fn compiler_versions_keep_historical_geometry_and_the_current_default() {
        for duration in [767_999, 768_000, 768_001] {
            let end = SongTime::from_frames(duration + 16);
            let sections = [section(0, duration)];
            let old = compile_version("versioned", end, &sections, 1).unwrap();
            let current = compile_version("versioned", end, &sections, 2).unwrap();
            assert_eq!(old.compiler_version(), 1);
            assert_eq!(current.compiler_version(), 2);
            assert_eq!(compile("versioned", end, &sections).unwrap(), current);
            assert_ne!(old, current);
            assert_eq!(old.segments().len(), 2);
            assert_eq!(old.segments()[0].kind, SegmentKind::Plaza);
            assert_eq!(old.segments()[0].end.frames(), duration);
            assert_eq!(old.segments()[1].kind, SegmentKind::Straight);
            if duration < 768_000 {
                assert_eq!(old.segments(), current.segments());
            } else {
                assert_eq!(current.segments().len(), 3);
                assert_eq!(current.segments()[0].kind, SegmentKind::Curve);
                assert_eq!(current.segments()[0].end.frames(), duration / 2);
                assert_eq!(current.segments()[1].kind, SegmentKind::Bridge);
                assert_eq!(current.segments()[1].end.frames(), duration);
            }
            for time in [SongTime::ZERO, SongTime::from_frames(duration), end] {
                assert_eq!(
                    old.sample(time),
                    current.sample(time).map(|mut sample| {
                        sample.kind = old.sample(time).unwrap().kind;
                        sample
                    })
                );
            }
            for time in [
                SongTime::from_frames(-1),
                SongTime::from_frames(duration + 17),
            ] {
                assert_eq!(old.sample(time), None);
                assert_eq!(current.sample(time), None);
            }
        }
        let end = SongTime::from_frames(768_000);
        let sections = [section(0, end.frames())];
        let old = compile_version("versioned", end, &sections, 1).unwrap();
        let current = compile_version("versioned", end, &sections, 2).unwrap();
        for (frame, width, kind, x, y) in [
            (0, 3_500, SegmentKind::Curve, 0, 0),
            (192_000, 3_750, SegmentKind::Curve, 600, 0),
            (384_000, 4_000, SegmentKind::Bridge, 0, 0),
            (576_000, 3_750, SegmentKind::Bridge, 0, 300),
            (768_000, 3_500, SegmentKind::Bridge, 0, 0),
        ] {
            let time = SongTime::from_frames(frame);
            assert_eq!(
                old.sample(time),
                Some(TrackSample {
                    distance_mm: frame / 16,
                    half_width_mm: width,
                    lateral_mm: 0,
                    elevation_mm: 0,
                    slope_x_ppm: 0,
                    slope_y_ppm: 0,
                    kind: SegmentKind::Plaza,
                })
            );
            let sample = current.sample(time).unwrap();
            assert_eq!(
                (
                    sample.half_width_mm,
                    sample.kind,
                    sample.lateral_mm,
                    sample.elevation_mm
                ),
                (3_500, kind, x, y)
            );
        }
        for version in [0, 3, u32::MAX] {
            assert!(compile_version("versioned", end, &sections, version).is_err());
        }
    }

    #[test]
    fn authored_intervals_gaps_and_eof_have_deterministic_geometry() {
        let end = SongTime::from_frames(256_000);
        let sections = [
            section(32_000, 96_000),
            section(96_000, 96_001),
            section(160_000, 256_000),
        ];
        let plan = compile("package-a", end, &sections).unwrap();
        assert_eq!(plan, compile("package-a", end, &sections).unwrap());
        assert_eq!(plan.content_id(), "package-a");
        assert_eq!(plan.end(), end);
        assert_eq!(plan.compiler_version(), 2);
        assert_eq!(
            plan.segments()
                .iter()
                .map(|segment| (segment.start.frames(), segment.end.frames(), segment.kind))
                .collect::<Vec<_>>(),
            [
                (0, 32_000, SegmentKind::Straight),
                (32_000, 96_000, SegmentKind::Plaza),
                (96_000, 96_001, SegmentKind::Plaza),
                (96_001, 160_000, SegmentKind::Straight),
                (160_000, 256_000, SegmentKind::Plaza),
            ]
        );
        for (frame, distance, width, kind) in [
            (0, 0, 3_500, SegmentKind::Straight),
            (31_999, 1_999, 3_500, SegmentKind::Straight),
            (32_000, 2_000, 3_500, SegmentKind::Plaza),
            (48_000, 3_000, 3_750, SegmentKind::Plaza),
            (64_000, 4_000, 4_000, SegmentKind::Plaza),
            (80_000, 5_000, 3_750, SegmentKind::Plaza),
            (95_999, 5_999, 3_500, SegmentKind::Plaza),
            (96_000, 6_000, 3_500, SegmentKind::Plaza),
            (96_001, 6_000, 3_500, SegmentKind::Straight),
            (160_000, 10_000, 3_500, SegmentKind::Plaza),
            (208_000, 13_000, 4_000, SegmentKind::Plaza),
            (256_000, 16_000, 3_500, SegmentKind::Plaza),
        ] {
            let expected = Some(TrackSample {
                distance_mm: distance,
                half_width_mm: width,
                lateral_mm: 0,
                elevation_mm: 0,
                slope_x_ppm: 0,
                slope_y_ppm: 0,
                kind,
            });
            assert_eq!(plan.sample(SongTime::from_frames(frame)), expected);
            assert_eq!(plan.sample(SongTime::from_frames(frame)), expected);
        }
        for frame in [i64::MIN, -1, 256_001, i64::MAX] {
            assert_eq!(plan.sample(SongTime::from_frames(frame)), None);
        }
        let mut relabelled = sections.clone();
        relabelled[0].label = "different author label".into();
        relabelled[0].confidence = Some(0.75);
        let relabelled = compile("package-b", end, &relabelled).unwrap();
        assert_eq!(plan.segments(), relabelled.segments());
        assert_eq!(plan.sample(end), relabelled.sample(end));
        assert_ne!(plan, relabelled);
    }

    #[test]
    fn quantized_short_sections_and_empty_analysis_keep_continuous_bounds() {
        for duration in [1, 2, 31, 32, 33, 320, 16_000, 16_001] {
            let end = SongTime::from_frames(duration);
            let plan = compile("short", end, &[section(0, duration)]).unwrap();
            let mut previous = plan.sample(SongTime::ZERO).unwrap();
            assert_eq!(previous.half_width_mm, 3_500);
            for frame in 0..=duration {
                let sample = plan.sample(SongTime::from_frames(frame)).unwrap();
                let reflected = plan
                    .sample(SongTime::from_frames(duration - frame))
                    .unwrap();
                assert_eq!(sample.half_width_mm, reflected.half_width_mm);
                assert!((sample.half_width_mm - previous.half_width_mm).abs() <= 1);
                assert!((3_500..=4_000).contains(&sample.half_width_mm));
                if duration < 32 {
                    assert_eq!(sample.half_width_mm, 3_500);
                }
                previous = sample;
            }
            assert_eq!(previous.half_width_mm, 3_500);
        }
        let end = SongTime::from_frames(MAX_CANONICAL_FRAMES as i64);
        let empty = compile("empty-analysis", end, &[]).unwrap();
        assert_eq!(empty.segments().len(), 1);
        assert_eq!(empty.segments()[0].start, SongTime::ZERO);
        assert_eq!(empty.segments()[0].end, end);
        assert_eq!(
            empty.sample(end),
            Some(TrackSample {
                distance_mm: 1_800_000,
                half_width_mm: 3_500,
                lateral_mm: 0,
                elevation_mm: 0,
                slope_x_ppm: 0,
                slope_y_ppm: 0,
                kind: SegmentKind::Straight,
            })
        );
    }

    #[test]
    fn curve_and_bridge_match_rational_reference_and_join_flat() {
        // Exact Bernstein-basis reference: position controls [0, 0, 8A/3, 0, 0],
        // derivative controls [0, 32A/3, -32A/3, 0] scaled by 16e6 / duration
        for (duration, frame, x, slope_x, y, slope_y) in [
            (384_000, 0, 0, 0, 0, 0),
            (384_000, 1, 0, 2, 0, 1),
            (384_000, 96_000, 338, 75_000, 169, 37_500),
            (384_000, 192_000, 600, 0, 300, 0),
            (384_000, 288_000, 338, -75_000, 169, -37_500),
            (384_000, 383_999, 0, -2, 0, -1),
            (384_000, 384_000, 0, 0, 0, 0),
            (384_001, 96_000, 337, 75_000, 169, 37_500),
            (384_001, 192_000, 600, 1, 300, 0),
            (384_001, 192_001, 600, -1, 300, 0),
            (384_001, 288_001, 337, -75_000, 169, -37_500),
            (1_024_000, 256_000, 338, 28_125, 169, 14_063),
            (1_024_000, 768_000, 338, -28_125, 169, -14_063),
            (2_048_000, 512_000, 338, 14_063, 169, 7_031),
            (2_048_000, 1_536_000, 338, -14_063, 169, -7_031),
            (14_400_000, 0, 0, 0, 0, 0),
            (14_400_000, 1, 0, 0, 0, 0),
            (14_400_000, 3_600_000, 338, 2_000, 169, 1_000),
            (14_400_000, 7_200_000, 600, 0, 300, 0),
            (14_400_000, 10_800_000, 338, -2_000, 169, -1_000),
            (14_400_000, 14_399_999, 0, 0, 0, 0),
            (14_400_000, 14_400_000, 0, 0, 0, 0),
        ] {
            let end = SongTime::from_frames(2 * duration);
            let plan = compile("reference", end, &[section(0, 2 * duration)]).unwrap();
            let curve = plan.sample(SongTime::from_frames(frame)).unwrap();
            let bridge = plan
                .sample(SongTime::from_frames(duration + frame))
                .unwrap();
            assert_eq!(
                (
                    curve.lateral_mm,
                    curve.elevation_mm,
                    curve.slope_x_ppm,
                    curve.slope_y_ppm
                ),
                (x, 0, slope_x, 0),
                "curve d={duration} n={frame}",
            );
            assert_eq!(
                (
                    bridge.lateral_mm,
                    bridge.elevation_mm,
                    bridge.slope_x_ppm,
                    bridge.slope_y_ppm
                ),
                (0, y, 0, slope_y),
                "bridge d={duration} n={frame}",
            );
            assert_eq!(curve.half_width_mm, 3_500);
            assert_eq!(bridge.half_width_mm, 3_500);
            assert_eq!(bridge.kind, SegmentKind::Bridge);
        }

        for (numerator, denominator, expected) in [
            (-5, 2, -3),
            (-3, 2, -2),
            (-1, 2, -1),
            (-499, 1_000, 0),
            (0, 1, 0),
            (499, 1_000, 0),
            (1, 2, 1),
            (3, 2, 2),
            (5, 2, 3),
            (-1, 3, 0),
            (-2, 3, -1),
            (1, 3, 0),
            (2, 3, 1),
        ] {
            assert_eq!(round_ratio(numerator, denominator), expected);
        }

        // Bound every intermediate product, even without using n(d-n) <= d²/4
        let d = i128::from(MAX_CANONICAL_FRAMES / 2);
        let denominator = d.pow(4);
        let position_bound = (16 * 600_i128).checked_mul(denominator).unwrap();
        let slope_bound = (512 * 600_i128 * 1_000_000).checked_mul(d.pow(3)).unwrap();
        assert_eq!(position_bound, 412_782_428_160_000_000_000_000_000_000_000);
        assert_eq!(slope_bound, 917_294_284_800_000_000_000_000_000_000_000);
        assert!(position_bound.checked_add(denominator / 2).is_some());
        assert!(slope_bound.checked_add(denominator / 2).is_some());

        let plan = compile(
            "shortest-features",
            SongTime::from_frames(768_000),
            &[section(0, 768_000)],
        )
        .unwrap();
        let mut previous = plan.sample(SongTime::ZERO).unwrap();
        let mut maxima = (0, 0);
        for frame in 1..=768_000 {
            let sample = plan.sample(SongTime::from_frames(frame)).unwrap();
            assert!((sample.lateral_mm - previous.lateral_mm).abs() <= 1);
            assert!((sample.elevation_mm - previous.elevation_mm).abs() <= 1);
            maxima.0 = maxima.0.max(sample.slope_x_ppm.abs());
            maxima.1 = maxima.1.max(sample.slope_y_ppm.abs());
            previous = sample;
        }
        assert_eq!(maxima, (76_980, 38_490));
    }

    #[test]
    fn thresholds_odd_split_and_dense_mixed_sections_keep_bounded_coverage() {
        let sections = [
            section(0, 767_999),
            section(768_002, 1_536_002),
            section(1_536_002, 2_304_003),
            section(2_304_003, 2_304_004),
        ];
        let end = SongTime::from_frames(2_304_020);
        let plan = compile("thresholds", end, &sections).unwrap();
        assert_eq!(
            plan.segments()
                .iter()
                .map(|s| (s.start.frames(), s.end.frames(), s.kind))
                .collect::<Vec<_>>(),
            [
                (0, 767_999, SegmentKind::Plaza),
                (767_999, 768_002, SegmentKind::Straight),
                (768_002, 1_152_002, SegmentKind::Curve),
                (1_152_002, 1_536_002, SegmentKind::Bridge),
                (1_536_002, 1_920_002, SegmentKind::Curve),
                (1_920_002, 2_304_003, SegmentKind::Bridge),
                (2_304_003, 2_304_004, SegmentKind::Plaza),
                (2_304_004, 2_304_020, SegmentKind::Straight),
            ],
        );
        let mut relabelled = sections;
        for section in &mut relabelled {
            section.label = "no inferred musical meaning".into();
            section.confidence = Some(0.75);
        }
        let relabelled = compile("other-identity", end, &relabelled).unwrap();
        assert_eq!(plan.segments(), relabelled.segments());
        for segment in plan.segments() {
            let midpoint =
                SongTime::from_frames((segment.start.frames() + segment.end.frames()) / 2);
            assert_eq!(plan.sample(midpoint), relabelled.sample(midpoint));
        }

        let short_count = MAX_CONTENT_ITEMS - 37;
        let mut sections: Vec<_> = (0..short_count)
            .map(|index| section(2 * index as i64 + 1, 2 * index as i64 + 2))
            .collect();
        let mut through = 2 * short_count as i64;
        for _ in 0..37 {
            sections.push(section(through + 1, through + 1 + 768_000));
            through += 1 + 768_000;
        }
        let end = SongTime::from_frames(MAX_CANONICAL_FRAMES as i64);
        let plan = compile("maximum-mixed", end, &sections).unwrap();
        assert_eq!(plan.segments().len(), 2 * MAX_CONTENT_ITEMS + 1 + 37);
        assert_eq!(plan.segments()[0].start, SongTime::ZERO);
        assert_eq!(plan.segments().last().unwrap().end, end);
        assert_eq!(
            plan.segments()
                .iter()
                .filter(|s| matches!(s.kind, SegmentKind::Curve | SegmentKind::Bridge))
                .count(),
            74
        );
        assert!(
            plan.segments()
                .windows(2)
                .all(|pair| pair[0].end == pair[1].start)
        );
        for segment in plan.segments() {
            assert!(segment.start < segment.end);
            for time in [segment.start, segment.end] {
                let sample = plan.sample(time).unwrap();
                assert_eq!(
                    (
                        sample.lateral_mm,
                        sample.elevation_mm,
                        sample.slope_x_ppm,
                        sample.slope_y_ppm
                    ),
                    (0, 0, 0, 0)
                );
                assert_eq!(sample.half_width_mm, 3_500);
            }
        }
    }

    #[test]
    fn invalid_intervals_are_rejected_and_maximum_input_has_bounded_coverage() {
        let end = SongTime::from_frames(1_000);
        for sections in [
            vec![section(-1, 10)],
            vec![section(0, 0)],
            vec![section(5, 4)],
            vec![section(0, 1_001)],
            vec![section(1_000, 1_001)],
            vec![section(0, 100), section(99, 200)],
            vec![section(100, 200), section(0, 50)],
            vec![section(i64::MIN, i64::MAX)],
        ] {
            assert!(compile("id", end, &sections).is_err());
        }
        for frames in [i64::MIN, -1, 0, MAX_CANONICAL_FRAMES as i64 + 1, i64::MAX] {
            assert!(compile("id", SongTime::from_frames(frames), &[]).is_err());
        }
        assert!(compile("", end, &[]).is_err());
        assert!(compile(&"é".repeat(128), end, &[]).is_ok());
        assert!(compile(&"é".repeat(129), end, &[]).is_err());

        let mut sections: Vec<_> = (0..MAX_CONTENT_ITEMS)
            .map(|index| section(2 * index as i64 + 1, 2 * index as i64 + 2))
            .collect();
        let end = SongTime::from_frames(2 * MAX_CONTENT_ITEMS as i64 + 1);
        let plan = compile("maximum", end, &sections).unwrap();
        assert_eq!(plan.segments().len(), 2 * MAX_CONTENT_ITEMS + 1);
        assert_eq!(plan.segments()[0].start, SongTime::ZERO);
        assert_eq!(plan.segments().last().unwrap().end, end);
        for segment in plan.segments() {
            assert!(segment.start < segment.end);
            assert_eq!(plan.sample(segment.start).unwrap().kind, segment.kind);
            assert_eq!(plan.sample(segment.start).unwrap().half_width_mm, 3_500);
            assert_eq!(plan.sample(segment.end).unwrap().half_width_mm, 3_500);
        }
        assert!(
            plan.segments()
                .windows(2)
                .all(|pair| pair[0].end == pair[1].start)
        );
        sections.push(section(end.frames() - 1, end.frames()));
        assert!(compile("oversized", end, &sections).is_err());
    }
}
