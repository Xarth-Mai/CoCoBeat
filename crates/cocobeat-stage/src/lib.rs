//! Deterministic song-time geometry, independent of rendering and music judgement

use cocobeat_schema::{
    MAX_CANONICAL_FRAMES, MAX_CONTENT_ITEMS, MAX_CONTENT_TEXT_BYTES, SectionFeature, SongTime,
};

pub const COMPILER_VERSION: u32 = 1;
pub const BASE_HALF_WIDTH_MM: i64 = 3_500;
const MAX_PLAZA_EXPANSION_MM: i64 = 500;
const FRAMES_PER_MM: i64 = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagePlan {
    content_id: String,
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrackSample {
    pub distance_mm: i64,
    pub half_width_mm: i64,
    pub kind: SegmentKind,
}

/// Compile real analysis intervals without inferring intervals from chart cues
pub fn compile(
    content_id: &str,
    end: SongTime,
    sections: &[SectionFeature],
) -> Result<StagePlan, String> {
    if content_id.is_empty() || content_id.len() > MAX_CONTENT_TEXT_BYTES {
        return Err("Stage content identity must contain 1 to 256 UTF-8 bytes".into());
    }
    if !(1..=MAX_CANONICAL_FRAMES as i64).contains(&end.frames()) {
        return Err("Stage must cover more than zero and at most ten minutes".into());
    }
    if sections.len() > MAX_CONTENT_ITEMS {
        return Err("Stage section count exceeds the content item limit".into());
    }

    let mut segments = Vec::with_capacity(sections.len() * 2 + 1);
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
        segments.push(TrackSegment {
            start: section.start,
            end: section.end,
            kind: SegmentKind::Plaza,
        });
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
        COMPILER_VERSION
    }

    pub fn segments(&self) -> &[TrackSegment] {
        &self.segments
    }

    /// Sample closed song bounds; the end retains the last kind at base width
    /// Distances and symmetric linear expansion both round down to millimetres
    pub fn sample(&self, time: SongTime) -> Option<TrackSample> {
        if time < SongTime::ZERO || time > self.end {
            return None;
        }
        let index = self.segments.partition_point(|segment| segment.end <= time);
        let segment = &self.segments[index.min(self.segments.len() - 1)];
        let expansion = match segment.kind {
            SegmentKind::Straight => 0,
            SegmentKind::Plaza => {
                let duration = segment.end.frames() - segment.start.frames();
                let elapsed = time.frames() - segment.start.frames();
                // Limit short sections to one millimetre of width per millimetre of travel
                let peak = MAX_PLAZA_EXPANSION_MM.min(duration / (2 * FRAMES_PER_MM));
                2 * peak * elapsed.min(duration - elapsed) / duration
            }
        };
        Some(TrackSample {
            distance_mm: time.frames() / FRAMES_PER_MM,
            half_width_mm: BASE_HALF_WIDTH_MM + expansion,
            kind: segment.kind,
        })
    }
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
        assert_eq!(plan.compiler_version(), 1);
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
                kind: SegmentKind::Straight,
            })
        );
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
