//! Experimental onset selection with source evidence, independent of playback and files

use cocobeat_schema::{Anchor, MAX_CANONICAL_FRAMES, MusicAnalysis, SongTime};
use std::collections::BTreeMap;

pub const ANCHOR_COMPILER_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorPolicy {
    pub min_confidence: f32,
    pub min_gap_frames: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnchorProposal {
    pub anchors: Vec<Anchor>,
    pub evidence: Vec<AnchorEvidence>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorEvidence {
    pub onset_index: usize,
    pub time: SongTime,
    pub strength: f32,
    pub confidence: Option<f32>,
    pub decision: AnchorDecision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnchorDecision {
    Selected {
        anchor_id: u64,
    },
    UnknownConfidence,
    BelowConfidence,
    TooClose {
        blocking_onset_index: usize,
        distance_frames: i64,
    },
}

/// Supplied confidence is compared as data, without asserting calibration or musical quality
pub fn compile_anchor_proposal(
    analysis: &MusicAnalysis,
    canonical_frames: u64,
    policy: AnchorPolicy,
) -> Result<AnchorProposal, String> {
    analysis.validate(canonical_frames)?;
    if !policy.min_confidence.is_finite()
        || policy.min_confidence <= 0.0
        || policy.min_confidence > 1.0
    {
        return Err("Anchor minimum confidence must be finite and within (0, 1]".into());
    }
    if !(1..=MAX_CANONICAL_FRAMES as i64).contains(&policy.min_gap_frames) {
        return Err("Anchor minimum gap must be 1 to 28800000 frames".into());
    }

    let mut evidence = Vec::with_capacity(analysis.onsets.len());
    let mut candidates = Vec::new();
    for (onset_index, onset) in analysis.onsets.iter().enumerate() {
        let decision = match onset.confidence {
            None => AnchorDecision::UnknownConfidence,
            Some(confidence) if confidence < policy.min_confidence => {
                AnchorDecision::BelowConfidence
            }
            Some(confidence) => {
                candidates.push((onset_index, confidence));
                AnchorDecision::Selected {
                    anchor_id: onset_index as u64 + 1,
                }
            }
        };
        evidence.push(AnchorEvidence {
            onset_index,
            time: onset.time,
            strength: onset.strength,
            confidence: onset.confidence,
            decision,
        });
    }
    candidates.sort_unstable_by(|&(left, left_confidence), &(right, right_confidence)| {
        right_confidence
            .total_cmp(&left_confidence)
            .then_with(|| evidence[left].time.cmp(&evidence[right].time))
    });

    let mut selected = BTreeMap::<SongTime, usize>::new();
    for (index, _) in candidates {
        let time = evidence[index].time;
        let blocker = selected
            .range(..time)
            .next_back()
            .into_iter()
            .chain(selected.range(time..).next())
            .map(|(&other_time, &other_index)| {
                (
                    (time.frames() - other_time.frames()).abs(),
                    other_time,
                    other_index,
                )
            })
            .filter(|&(distance, _, _)| distance < policy.min_gap_frames)
            .min();
        if let Some((distance_frames, _, blocking_onset_index)) = blocker {
            evidence[index].decision = AnchorDecision::TooClose {
                blocking_onset_index,
                distance_frames,
            };
        } else {
            selected.insert(time, index);
        }
    }
    let anchors = selected
        .into_iter()
        .map(|(song_time, index)| Anchor {
            id: index as u64 + 1,
            song_time,
        })
        .collect();
    Ok(AnchorProposal { anchors, evidence })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cocobeat_schema::{
        BeatFeature, EnergySample, MAX_CONTENT_ITEMS, OnsetFeature, SectionFeature,
    };

    fn analysis(frames: u32, points: &[(i64, Option<f32>)]) -> MusicAnalysis {
        MusicAnalysis {
            schema_version: 1,
            audio_hash: [7; 32],
            beats: Vec::new(),
            onsets: points
                .iter()
                .enumerate()
                .map(|(index, &(frame, confidence))| OnsetFeature {
                    time: SongTime::from_frames(frame),
                    strength: index as f32,
                    confidence,
                })
                .collect(),
            sections: Vec::new(),
            energy: vec![EnergySample {
                start: SongTime::ZERO,
                frames,
                rms: [0.0; 2],
                peak: [0.0; 2],
            }],
            diagnostics: "Constructed scores test selection mechanics, not calibrated MIR".into(),
        }
    }

    #[test]
    fn ranked_selection_checks_both_neighbors_and_keeps_source_ids() {
        let input = analysis(
            100,
            &[
                (0, None),
                (1, Some(0.49)),
                (10, Some(0.7)),
                (20, Some(0.9)),
                (29, Some(0.8)),
                (30, Some(0.7)),
                (40, Some(0.9)),
                (45, Some(0.6)),
                (59, Some(0.8)),
                (60, Some(0.5)),
                (99, Some(0.5)),
            ],
        );
        let policy = AnchorPolicy {
            min_confidence: 0.5,
            min_gap_frames: 20,
        };
        let proposal = compile_anchor_proposal(&input, 100, policy).unwrap();
        assert_eq!(
            proposal,
            compile_anchor_proposal(&input, 100, policy).unwrap()
        );
        assert_eq!(
            proposal.anchors,
            [(4, 20), (7, 40), (10, 60), (11, 99)].map(|(id, frame)| Anchor {
                id,
                song_time: SongTime::from_frames(frame)
            })
        );
        assert_eq!(
            proposal
                .evidence
                .iter()
                .map(|item| item.decision)
                .collect::<Vec<_>>(),
            [
                AnchorDecision::UnknownConfidence,
                AnchorDecision::BelowConfidence,
                AnchorDecision::TooClose {
                    blocking_onset_index: 3,
                    distance_frames: 10
                },
                AnchorDecision::Selected { anchor_id: 4 },
                AnchorDecision::TooClose {
                    blocking_onset_index: 3,
                    distance_frames: 9
                },
                AnchorDecision::TooClose {
                    blocking_onset_index: 3,
                    distance_frames: 10
                },
                AnchorDecision::Selected { anchor_id: 7 },
                AnchorDecision::TooClose {
                    blocking_onset_index: 6,
                    distance_frames: 5
                },
                AnchorDecision::TooClose {
                    blocking_onset_index: 6,
                    distance_frames: 19
                },
                AnchorDecision::Selected { anchor_id: 10 },
                AnchorDecision::Selected { anchor_id: 11 },
            ]
        );
        for (index, (item, onset)) in proposal.evidence.iter().zip(&input.onsets).enumerate() {
            assert_eq!(item.onset_index, index);
            assert_eq!(
                (item.time, item.strength, item.confidence),
                (onset.time, onset.strength, onset.confidence)
            );
        }
        let stricter = compile_anchor_proposal(
            &input,
            100,
            AnchorPolicy {
                min_confidence: 0.85,
                ..policy
            },
        )
        .unwrap();
        assert_eq!(stricter.anchors, proposal.anchors[..2]);
        let wider = compile_anchor_proposal(
            &input,
            100,
            AnchorPolicy {
                min_gap_frames: 1_000,
                ..policy
            },
        )
        .unwrap();
        assert_eq!(wider.anchors, proposal.anchors[..1]);
        let right_is_closer = compile_anchor_proposal(
            &analysis(40, &[(0, Some(0.9)), (12, Some(0.7)), (20, Some(0.9))]),
            40,
            AnchorPolicy {
                min_gap_frames: 15,
                ..policy
            },
        )
        .unwrap();
        assert_eq!(
            right_is_closer.evidence[1].decision,
            AnchorDecision::TooClose {
                blocking_onset_index: 2,
                distance_frames: 8,
            }
        );
    }

    #[test]
    fn only_onset_scores_and_times_select_and_unknown_stays_empty() {
        let policy = AnchorPolicy {
            min_confidence: 0.5,
            min_gap_frames: 10,
        };
        let mut input = analysis(100, &[(0, Some(0.5)), (99, Some(1.0))]);
        let original = compile_anchor_proposal(&input, 100, policy).unwrap();
        input.beats.push(BeatFeature {
            time: SongTime::from_frames(30),
            strength: 99.0,
            downbeat_probability: Some(1.0),
            confidence: Some(1.0),
        });
        input.sections.push(SectionFeature {
            start: SongTime::ZERO,
            end: SongTime::from_frames(100),
            confidence: Some(1.0),
            label: "high energy".into(),
        });
        input.energy[0].rms = [0.5; 2];
        input.energy[0].peak = [1.0; 2];
        assert_eq!(
            compile_anchor_proposal(&input, 100, policy).unwrap(),
            original
        );
        input.onsets[0].strength = f32::MAX;
        assert_eq!(
            compile_anchor_proposal(&input, 100, policy)
                .unwrap()
                .anchors,
            original.anchors
        );
        input.onsets[0].time = SongTime::from_frames(1);
        let moved = compile_anchor_proposal(&input, 100, policy).unwrap();
        assert_eq!(
            moved.anchors[0],
            Anchor {
                id: 1,
                song_time: SongTime::from_frames(1)
            }
        );
        input.onsets[0].confidence = Some(0.49);
        assert_eq!(
            compile_anchor_proposal(&input, 100, policy)
                .unwrap()
                .anchors,
            original.anchors[1..]
        );
        input.onsets[1].confidence = None;
        let empty = compile_anchor_proposal(&input, 100, policy).unwrap();
        assert!(empty.anchors.is_empty());
        assert_eq!(empty.evidence.len(), 2);
        input.onsets.clear();
        assert_eq!(
            compile_anchor_proposal(&input, 100, policy).unwrap(),
            AnchorProposal {
                anchors: vec![],
                evidence: vec![]
            }
        );
        input.sections[0].label.clear();
        assert!(compile_anchor_proposal(&input, 100, policy).is_err());
    }

    #[test]
    fn invalid_source_or_policy_is_rejected_without_repair() {
        let input = analysis(100, &[(0, Some(0.5)), (99, Some(1.0))]);
        let policy = AnchorPolicy {
            min_confidence: 0.5,
            min_gap_frames: 10,
        };
        for confidence in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -1.0,
            0.0,
            1.0 + f32::EPSILON,
        ] {
            assert!(
                compile_anchor_proposal(
                    &input,
                    100,
                    AnchorPolicy {
                        min_confidence: confidence,
                        ..policy
                    }
                )
                .is_err()
            );
        }
        for gap in [i64::MIN, -1, 0, MAX_CANONICAL_FRAMES as i64 + 1, i64::MAX] {
            assert!(
                compile_anchor_proposal(
                    &input,
                    100,
                    AnchorPolicy {
                        min_gap_frames: gap,
                        ..policy
                    }
                )
                .is_err()
            );
        }
        for frames in [0, MAX_CANONICAL_FRAMES + 1, u64::MAX] {
            assert!(compile_anchor_proposal(&input, frames, policy).is_err());
        }
        for frame in [i64::MIN, -1, 100, i64::MAX] {
            let mut bad = input.clone();
            bad.onsets[0].time = SongTime::from_frames(frame);
            assert!(compile_anchor_proposal(&bad, 100, policy).is_err());
        }
        let mut bad = input.clone();
        bad.onsets.reverse();
        assert!(compile_anchor_proposal(&bad, 100, policy).is_err());
        bad.onsets[0].time = bad.onsets[1].time;
        assert!(compile_anchor_proposal(&bad, 100, policy).is_err());
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
            let mut bad = input.clone();
            bad.onsets[0].strength = value;
            assert!(compile_anchor_proposal(&bad, 100, policy).is_err());
            bad = input.clone();
            bad.onsets[0].confidence = Some(value);
            assert!(compile_anchor_proposal(&bad, 100, policy).is_err());
        }
        let mut bad = input.clone();
        bad.onsets[0].confidence = Some(1.0 + f32::EPSILON);
        assert!(compile_anchor_proposal(&bad, 100, policy).is_err());
        let mut bad = input.clone();
        bad.energy.clear();
        assert!(compile_anchor_proposal(&bad, 100, policy).is_err());
        assert!(
            compile_anchor_proposal(
                &input,
                100,
                AnchorPolicy {
                    min_confidence: f32::from_bits(1),
                    ..policy
                }
            )
            .is_ok()
        );
        assert_eq!(
            compile_anchor_proposal(
                &input,
                100,
                AnchorPolicy {
                    min_confidence: 1.0,
                    ..policy
                }
            )
            .unwrap()
            .anchors[0]
                .id,
            2
        );
    }

    #[test]
    fn maximum_input_keeps_every_evidence_item_and_exact_gap_bounds() {
        let points: Vec<_> = (0..MAX_CONTENT_ITEMS)
            .map(|index| (index as i64, Some(0.5)))
            .collect();
        let mut input = analysis(MAX_CANONICAL_FRAMES as u32, &points);
        let policy = AnchorPolicy {
            min_confidence: 0.5,
            min_gap_frames: 1,
        };
        let all = compile_anchor_proposal(&input, MAX_CANONICAL_FRAMES, policy).unwrap();
        assert_eq!(all.anchors.len(), 100_000);
        assert_eq!(all.evidence.len(), 100_000);
        assert_eq!(
            all.anchors.first(),
            Some(&Anchor {
                id: 1,
                song_time: SongTime::ZERO
            })
        );
        assert_eq!(
            all.anchors.last(),
            Some(&Anchor {
                id: 100_000,
                song_time: SongTime::from_frames(99_999)
            })
        );
        let sparse = compile_anchor_proposal(
            &input,
            MAX_CANONICAL_FRAMES,
            AnchorPolicy {
                min_gap_frames: 3,
                ..policy
            },
        )
        .unwrap();
        assert_eq!(sparse.anchors.len(), 33_334);
        assert_eq!(sparse.evidence.len(), 100_000);
        for (index, anchor) in sparse.anchors.iter().enumerate() {
            assert_eq!(
                (anchor.id, anchor.song_time.frames()),
                ((index * 3 + 1) as u64, (index * 3) as i64)
            );
        }
        let one = compile_anchor_proposal(
            &input,
            MAX_CANONICAL_FRAMES,
            AnchorPolicy {
                min_gap_frames: MAX_CANONICAL_FRAMES as i64,
                ..policy
            },
        )
        .unwrap();
        assert_eq!(one.anchors, all.anchors[..1]);
        input.onsets.push(OnsetFeature {
            time: SongTime::from_frames(100_000),
            strength: 0.0,
            confidence: None,
        });
        assert!(compile_anchor_proposal(&input, MAX_CANONICAL_FRAMES, policy).is_err());
    }
}
