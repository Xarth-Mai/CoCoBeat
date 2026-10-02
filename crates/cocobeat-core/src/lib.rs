//! 确定性双人规则：真实输入经双方进度水位确认，展示层只消费结果

use cocobeat_schema::{
    Anchor, AnchorGrade, AnchorJudgement, AnchorSyncEvent, DuoEvent, DuoInput, DuoRules,
    FreeSyncEvent, Hit, PlayerId, ResonanceState, SessionEpoch, SongTime,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DuoError {
    InvalidRules(&'static str),
    DuplicateAnchor(u64),
    TimeOverflow,
    EpochMismatch {
        expected: SessionEpoch,
        actual: SessionEpoch,
    },
    DuplicateHit {
        player: PlayerId,
        seq: u64,
    },
    ConflictingHit {
        player: PlayerId,
        seq: u64,
    },
    ClosedHistory {
        player: PlayerId,
        song_time: SongTime,
        through: SongTime,
    },
    WatermarkRegression {
        player: PlayerId,
        current: SongTime,
        requested: SongTime,
    },
}

impl fmt::Display for DuoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for DuoError {}

#[derive(Clone, Debug)]
struct InputState {
    hit: Hit,
    judged: bool,
    shared: bool,
}

#[derive(Clone, Debug)]
/// Anchor 按时间及 id 处理，候选按距离、时间及 seq 决定
/// Free Sync 先处理最早输入，等时按玩家及 seq 排序，再选最近同伴输入
/// 事件按逻辑确认时刻输出；到达顺序和水位批次不改变最终事件顺序
pub struct DuoEngine {
    epoch: SessionEpoch,
    rules: DuoRules,
    delay: i64,
    anchors: Vec<Anchor>,
    next_anchor: usize,
    inputs: BTreeMap<(PlayerId, u64), InputState>,
    watermarks: [Option<SongTime>; 2],
    events: Vec<DuoEvent>,
    pairs: Vec<(Hit, Hit)>,
}

impl DuoEngine {
    pub fn new(
        epoch: SessionEpoch,
        mut anchors: Vec<Anchor>,
        rules: DuoRules,
    ) -> Result<Self, DuoError> {
        if rules.precise_window_frames < 0
            || rules.good_window_frames < rules.precise_window_frames
            || rules.anchor_window_frames < rules.good_window_frames
            || rules.free_sync_window_frames < 0
            || rules.anchor_sync_window_frames < 0
            || rules.resonance_window_frames <= 0
            || rules.resonance_full_pairs == 0
        {
            return Err(DuoError::InvalidRules(
                "窗口须有序非负，Resonance 窗口和饱和值须为正",
            ));
        }
        let delay = rules
            .confirmation_delay_frames()
            .ok_or(DuoError::TimeOverflow)?;
        let mut ids = BTreeSet::new();
        for anchor in &anchors {
            if !ids.insert(anchor.id) {
                return Err(DuoError::DuplicateAnchor(anchor.id));
            }
            anchor
                .song_time
                .checked_add_frames(rules.anchor_window_frames)
                .ok_or(DuoError::TimeOverflow)?;
        }
        anchors.sort_by_key(|anchor| (anchor.song_time, anchor.id));
        Ok(Self {
            epoch,
            rules,
            delay,
            anchors,
            next_anchor: 0,
            inputs: BTreeMap::new(),
            watermarks: [None, None],
            events: Vec::new(),
            pairs: Vec::new(),
        })
    }

    /// 错误均在状态写入之前返回，重复输入与同身份冲突分开报告
    pub fn ingest(&mut self, input: DuoInput) -> Result<Vec<DuoEvent>, DuoError> {
        let epoch = match input {
            DuoInput::Hit(hit) => hit.epoch,
            DuoInput::Watermark { epoch, .. } => epoch,
        };
        if epoch != self.epoch {
            return Err(DuoError::EpochMismatch {
                expected: self.epoch,
                actual: epoch,
            });
        }
        let previous_events = self.events.len();
        match input {
            DuoInput::Hit(hit) => {
                let identity = (hit.player, hit.seq);
                if let Some(existing) = self.inputs.get(&identity) {
                    return Err(if existing.hit == hit {
                        DuoError::DuplicateHit {
                            player: hit.player,
                            seq: hit.seq,
                        }
                    } else {
                        DuoError::ConflictingHit {
                            player: hit.player,
                            seq: hit.seq,
                        }
                    });
                }
                if let Some(through) = self.watermarks[hit.player.index()]
                    && hit.song_time <= through
                {
                    return Err(DuoError::ClosedHistory {
                        player: hit.player,
                        song_time: hit.song_time,
                        through,
                    });
                }
                hit.song_time
                    .checked_add_frames(self.delay)
                    .ok_or(DuoError::TimeOverflow)?;
                self.inputs.insert(
                    identity,
                    InputState {
                        hit,
                        judged: false,
                        shared: false,
                    },
                );
            }
            DuoInput::Watermark {
                player, through, ..
            } => {
                if let Some(current) = self.watermarks[player.index()]
                    && through < current
                {
                    return Err(DuoError::WatermarkRegression {
                        player,
                        current,
                        requested: through,
                    });
                }
                self.watermarks[player.index()] = Some(through);
                self.resolve();
            }
        }
        Ok(self.events[previous_events..].to_vec())
    }

    pub fn events(&self) -> &[DuoEvent] {
        &self.events
    }
    pub fn confirmation_delay_frames(&self) -> i64 {
        self.delay
    }

    pub fn resonance(&self) -> ResonanceState {
        let Some(through) = self
            .common_watermark()
            .and_then(|time| time.checked_add_frames(-self.delay))
        else {
            return ResonanceState::default();
        };
        let in_window = |time: SongTime| {
            time <= through
                && i128::from(time.frames())
                    > i128::from(through.frames()) - i128::from(self.rules.resonance_window_frames)
        };
        let mut state = ResonanceState {
            through: Some(through),
            ..ResonanceState::default()
        };
        for input in self
            .inputs
            .values()
            .filter(|input| in_window(input.hit.song_time))
        {
            state.inputs[input.hit.player.index()] += 1;
        }
        state.matched_pairs = self
            .pairs
            .iter()
            .filter(|(p1, p2)| in_window(p1.song_time) && in_window(p2.song_time))
            .count() as u64;
        let total = u128::from(state.inputs[0]) + u128::from(state.inputs[1]);
        state.mutual_match_per_mille = (u128::from(state.matched_pairs) * 2_000)
            .checked_div(total)
            .unwrap_or(0) as u16;
        state.activity_per_mille = (u128::from(
            state
                .matched_pairs
                .min(u64::from(self.rules.resonance_full_pairs)),
        ) * 1_000
            / u128::from(self.rules.resonance_full_pairs))
            as u16;
        state.level_per_mille = (u32::from(state.mutual_match_per_mille)
            * u32::from(state.activity_per_mille)
            / 1_000) as u16;
        state
    }

    fn common_watermark(&self) -> Option<SongTime> {
        Some(self.watermarks[0]?.min(self.watermarks[1]?))
    }

    fn resolve(&mut self) {
        let Some(through) = self.common_watermark() else {
            return;
        };
        loop {
            // ponytail: 64 秒原型保留输入并线性扫描，长会话实测瓶颈后改为时间索引和历史回收
            let first = self
                .inputs
                .values()
                .filter(|input| !input.shared)
                .map(|input| input.hit)
                .min_by_key(|hit| (hit.song_time, hit.player, hit.seq));
            let free_at = first.map(|hit| hit.song_time.frames() + self.delay);
            let anchor_at = self
                .anchors
                .get(self.next_anchor)
                .map(|anchor| anchor.song_time.frames() + self.rules.anchor_window_frames);
            if let Some(at) = anchor_at
                && at <= through.frames()
                && free_at.is_none_or(|free_at| at <= free_at)
            {
                self.resolve_anchor(self.anchors[self.next_anchor]);
                self.next_anchor += 1;
            } else if let Some(at) = free_at
                && at <= through.frames()
            {
                self.resolve_free(first.expect("free_at 来自同一输入"));
            } else {
                break;
            }
        }
    }

    fn resolve_anchor(&mut self, anchor: Anchor) {
        let mut judgements = [AnchorJudgement {
            player: PlayerId::P1,
            anchor_id: anchor.id,
            input_seq: None,
            offset_frames: None,
            grade: AnchorGrade::Miss,
        }; 2];
        let mut selected = [None; 2];
        for player in [PlayerId::P1, PlayerId::P2] {
            let candidate = self
                .inputs
                .values()
                .filter(|input| input.hit.player == player && !input.judged)
                .filter(|input| {
                    distance(input.hit.song_time, anchor.song_time)
                        <= self.rules.anchor_window_frames as u64
                })
                .min_by_key(|input| {
                    (
                        distance(input.hit.song_time, anchor.song_time),
                        input.hit.song_time,
                        input.hit.seq,
                    )
                })
                .map(|input| input.hit);
            let judgement = &mut judgements[player.index()];
            judgement.player = player;
            if let Some(hit) = candidate {
                self.inputs
                    .get_mut(&(player, hit.seq))
                    .expect("候选已存在")
                    .judged = true;
                let offset = hit.song_time.frames() - anchor.song_time.frames();
                let absolute = offset.unsigned_abs();
                judgement.input_seq = Some(hit.seq);
                judgement.offset_frames = Some(offset);
                judgement.grade = if absolute <= self.rules.precise_window_frames as u64 {
                    AnchorGrade::Precise
                } else if absolute <= self.rules.good_window_frames as u64 {
                    AnchorGrade::Good
                } else {
                    AnchorGrade::LateOrEarly
                };
                selected[player.index()] = Some(hit);
            }
            self.events.push(DuoEvent::AnchorJudged(*judgement));
        }
        if let [Some(p1), Some(p2)] = selected
            && judgements.iter().all(|judgement| {
                matches!(judgement.grade, AnchorGrade::Precise | AnchorGrade::Good)
            })
            && distance(p1.song_time, p2.song_time) <= self.rules.anchor_sync_window_frames as u64
        {
            self.share(p1, p2);
            self.events.push(DuoEvent::AnchorSync(AnchorSyncEvent {
                anchor_id: anchor.id,
                p1: judgements[0],
                p2: judgements[1],
                relative_delta_frames: distance(p1.song_time, p2.song_time),
            }));
        }
    }

    fn resolve_free(&mut self, first: Hit) {
        let candidate = self
            .inputs
            .values()
            .filter(|input| !input.shared && input.hit.player != first.player)
            .filter(|input| {
                distance(input.hit.song_time, first.song_time)
                    <= self.rules.free_sync_window_frames as u64
            })
            .min_by_key(|input| {
                (
                    distance(input.hit.song_time, first.song_time),
                    input.hit.song_time,
                    input.hit.seq,
                )
            })
            .map(|input| input.hit);
        self.inputs
            .get_mut(&(first.player, first.seq))
            .expect("首输入已存在")
            .shared = true;
        if let Some(other) = candidate {
            let (p1, p2) = if first.player == PlayerId::P1 {
                (first, other)
            } else {
                (other, first)
            };
            self.share(p1, p2);
            self.events.push(DuoEvent::FreeSync(FreeSyncEvent {
                p1_input: p1.seq,
                p2_input: p2.seq,
                delta_frames: distance(p1.song_time, p2.song_time),
                midpoint: SongTime::from_frames(
                    (i128::from(p1.song_time.frames()) + i128::from(p2.song_time.frames()))
                        .div_euclid(2) as i64,
                ),
            }));
        }
    }

    fn share(&mut self, p1: Hit, p2: Hit) {
        for hit in [p1, p2] {
            self.inputs
                .get_mut(&(hit.player, hit.seq))
                .expect("配对输入已存在")
                .shared = true;
        }
        self.pairs.push((p1, p2));
    }
}

fn distance(left: SongTime, right: SongTime) -> u64 {
    left.frames().abs_diff(right.frames())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPOCH: SessionEpoch = SessionEpoch(7);

    fn rules() -> DuoRules {
        DuoRules {
            precise_window_frames: 2,
            good_window_frames: 5,
            anchor_window_frames: 10,
            free_sync_window_frames: 5,
            anchor_sync_window_frames: 5,
            resonance_window_frames: 100,
            resonance_full_pairs: 4,
        }
    }

    fn hit(player: PlayerId, seq: u64, at: i64) -> Hit {
        Hit {
            epoch: EPOCH,
            player,
            seq,
            song_time: SongTime::from_frames(at),
        }
    }

    fn engine(anchors: &[(u64, i64)]) -> DuoEngine {
        DuoEngine::new(
            EPOCH,
            anchors
                .iter()
                .map(|&(id, at)| Anchor {
                    id,
                    song_time: SongTime::from_frames(at),
                })
                .collect(),
            rules(),
        )
        .unwrap()
    }

    fn close(engine: &mut DuoEngine, through: i64) {
        for player in [PlayerId::P1, PlayerId::P2] {
            engine
                .ingest(DuoInput::Watermark {
                    epoch: EPOCH,
                    player,
                    through: SongTime::from_frames(through),
                })
                .unwrap();
        }
    }

    fn free_pairs(engine: &DuoEngine) -> Vec<(u64, u64)> {
        engine
            .events()
            .iter()
            .filter_map(|event| match event {
                DuoEvent::FreeSync(event) => Some((event.p1_input, event.p2_input)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn anchor_edges_are_inclusive_and_miss_waits_for_both_watermarks() {
        for (offset, expected) in [
            (-11, AnchorGrade::Miss),
            (-10, AnchorGrade::LateOrEarly),
            (-5, AnchorGrade::Good),
            (-2, AnchorGrade::Precise),
            (0, AnchorGrade::Precise),
            (2, AnchorGrade::Precise),
            (5, AnchorGrade::Good),
            (10, AnchorGrade::LateOrEarly),
            (11, AnchorGrade::Miss),
        ] {
            let mut engine = engine(&[(1, 100)]);
            engine
                .ingest(DuoInput::Hit(hit(PlayerId::P1, 1, 100 + offset)))
                .unwrap();
            close(&mut engine, 109);
            assert!(engine.events().is_empty());
            engine
                .ingest(DuoInput::Watermark {
                    epoch: EPOCH,
                    player: PlayerId::P1,
                    through: SongTime::from_frames(110),
                })
                .unwrap();
            assert!(engine.events().is_empty());
            close(&mut engine, 110);
            assert!(
                matches!(engine.events()[0], DuoEvent::AnchorJudged(AnchorJudgement { grade, player: PlayerId::P1, .. }) if grade == expected)
            );
            assert!(matches!(
                engine.events()[1],
                DuoEvent::AnchorJudged(AnchorJudgement {
                    grade: AnchorGrade::Miss,
                    player: PlayerId::P2,
                    ..
                })
            ));
            close(&mut engine, 1_000);
            assert_eq!(engine.events().len(), 2);
        }
    }

    #[test]
    fn free_pair_edges_ties_and_one_to_one_consumption_are_stable() {
        let mut engine = engine(&[]);
        for input in [
            hit(PlayerId::P2, 9, 105),
            hit(PlayerId::P1, 1, 100),
            hit(PlayerId::P2, 8, 95),
            hit(PlayerId::P2, 2, 95),
            hit(PlayerId::P1, 2, 200),
            hit(PlayerId::P2, 3, 206),
        ] {
            engine.ingest(DuoInput::Hit(input)).unwrap();
        }
        close(&mut engine, 500);
        assert_eq!(free_pairs(&engine), vec![(1, 2)]);
        assert!(
            matches!(engine.events()[0], DuoEvent::FreeSync(FreeSyncEvent { delta_frames: 5, midpoint, .. }) if midpoint.frames() == 97)
        );
    }

    #[test]
    fn anchors_consume_each_players_input_once_with_time_then_seq_ties() {
        let mut engine = engine(&[(3, 100), (2, 100), (1, 100)]);
        for input in [
            hit(PlayerId::P1, 9, 90),
            hit(PlayerId::P1, 2, 110),
            hit(PlayerId::P1, 3, 90),
        ] {
            engine.ingest(DuoInput::Hit(input)).unwrap();
        }
        close(&mut engine, 140);
        let chosen: Vec<_> = engine
            .events()
            .iter()
            .filter_map(|event| match event {
                DuoEvent::AnchorJudged(judgement) if judgement.player == PlayerId::P1 => {
                    Some((judgement.anchor_id, judgement.input_seq))
                }
                _ => None,
            })
            .collect();
        assert_eq!(chosen, vec![(1, Some(3)), (2, Some(9)), (3, Some(2))]);
    }

    #[test]
    fn anchor_rewards_have_priority_without_double_counting_and_lateness_is_not_accuracy() {
        let mut on_time = engine(&[(1, 100)]);
        for player in [PlayerId::P1, PlayerId::P2] {
            on_time.ingest(DuoInput::Hit(hit(player, 1, 100))).unwrap();
        }
        close(&mut on_time, 125);
        assert_eq!(
            on_time
                .events()
                .iter()
                .filter(|event| matches!(event, DuoEvent::AnchorSync(_)))
                .count(),
            1
        );
        assert!(free_pairs(&on_time).is_empty());
        assert_eq!(on_time.resonance().matched_pairs, 1);

        let mut late = engine(&[(1, 100)]);
        for player in [PlayerId::P1, PlayerId::P2] {
            late.ingest(DuoInput::Hit(hit(player, 1, 108))).unwrap();
        }
        close(&mut late, 133);
        assert!(
            late.events()
                .iter()
                .all(|event| !matches!(event, DuoEvent::AnchorSync(_)))
        );
        assert_eq!(free_pairs(&late), vec![(1, 1)]);
        assert_eq!(late.resonance().matched_pairs, 1);

        let mut separate = engine(&[(1, 100)]);
        for input in [hit(PlayerId::P1, 1, 95), hit(PlayerId::P2, 1, 105)] {
            separate.ingest(DuoInput::Hit(input)).unwrap();
        }
        close(&mut separate, 140);
        assert!(
            separate
                .events()
                .iter()
                .all(|event| !matches!(event, DuoEvent::AnchorSync(_)))
        );
    }

    #[test]
    fn all_720_delivery_orders_and_watermark_batches_have_identical_facts() {
        let inputs = [
            hit(PlayerId::P1, 1, 0),
            hit(PlayerId::P2, 1, 5),
            hit(PlayerId::P1, 2, 95),
            hit(PlayerId::P2, 2, 100),
            hit(PlayerId::P1, 3, 130),
            hit(PlayerId::P2, 3, 134),
        ];
        let mut baseline = engine(&[(1, 100), (2, 115)]);
        for input in inputs {
            baseline.ingest(DuoInput::Hit(input)).unwrap();
        }
        close(&mut baseline, 160);
        for rank in 0..720 {
            let mut remaining = inputs.to_vec();
            let mut divisor = rank;
            let mut reordered = engine(&[(2, 115), (1, 100)]);
            while !remaining.is_empty() {
                let index = divisor % remaining.len();
                divisor /= remaining.len();
                reordered
                    .ingest(DuoInput::Hit(remaining.remove(index)))
                    .unwrap();
            }
            for through in [25, 30, 110, 120, 125, 159, 160] {
                close(&mut reordered, through);
            }
            assert_eq!(reordered.events(), baseline.events(), "permutation {rank}");
            assert_eq!(
                reordered.resonance(),
                baseline.resonance(),
                "permutation {rank}"
            );
        }
        let mut streamed = engine(&[(1, 100), (2, 115)]);
        for chunk in inputs.chunks(2) {
            for input in chunk.iter().rev() {
                streamed.ingest(DuoInput::Hit(*input)).unwrap();
            }
            close(
                &mut streamed,
                chunk
                    .iter()
                    .map(|input| input.song_time.frames())
                    .max()
                    .unwrap(),
            );
        }
        close(&mut streamed, 160);
        assert_eq!(streamed.events(), baseline.events());
        assert_eq!(streamed.resonance(), baseline.resonance());
    }

    #[test]
    fn future_anchor_candidates_cannot_rewrite_earlier_free_rewards() {
        let mut engine = engine(&[(1, 20)]);
        for input in [hit(PlayerId::P1, 1, 0), hit(PlayerId::P2, 1, 5)] {
            engine.ingest(DuoInput::Hit(input)).unwrap();
        }
        close(&mut engine, 5);
        assert!(engine.events().is_empty());
        for input in [hit(PlayerId::P1, 2, 20), hit(PlayerId::P2, 2, 20)] {
            engine.ingest(DuoInput::Hit(input)).unwrap();
        }
        close(&mut engine, 50);
        assert_eq!(free_pairs(&engine), vec![(1, 1)]);
        assert_eq!(
            engine
                .events()
                .iter()
                .filter(|event| matches!(event, DuoEvent::AnchorSync(_)))
                .count(),
            1
        );
    }

    #[test]
    fn silence_spam_and_window_expiry_cannot_inflate_resonance() {
        let mut silent = engine(&[]);
        close(&mut silent, 100);
        assert!(silent.events().is_empty());
        assert_eq!(silent.resonance().level_per_mille, 0);

        let mut mutual = engine(&[]);
        for seq in 0..100 {
            for player in [PlayerId::P1, PlayerId::P2] {
                mutual
                    .ingest(DuoInput::Hit(hit(player, seq, seq as i64)))
                    .unwrap();
            }
        }
        close(&mut mutual, 124);
        assert_eq!(mutual.resonance().matched_pairs, 100);
        assert_eq!(mutual.resonance().level_per_mille, 1_000);
        close(&mut mutual, 224);
        assert_eq!(mutual.resonance().inputs, [0, 0]);
        assert_eq!(mutual.resonance().level_per_mille, 0);

        let mut one_sided = engine(&[]);
        one_sided
            .ingest(DuoInput::Hit(hit(PlayerId::P2, 0, 0)))
            .unwrap();
        for seq in 0..100 {
            one_sided
                .ingest(DuoInput::Hit(hit(PlayerId::P1, seq, seq as i64)))
                .unwrap();
        }
        close(&mut one_sided, 124);
        assert_eq!(one_sided.resonance().matched_pairs, 1);
        assert!(one_sided.resonance().level_per_mille < 10);
    }

    #[test]
    fn invalid_input_and_watermarks_leave_state_unchanged() {
        let mut engine = engine(&[(1, 100)]);
        let original = hit(PlayerId::P1, 1, 100);
        engine.ingest(DuoInput::Hit(original)).unwrap();
        close(&mut engine, 100);
        let mut unchanged = engine.clone();
        assert!(matches!(
            engine.ingest(DuoInput::Hit(original)),
            Err(DuoError::DuplicateHit { .. })
        ));
        assert!(matches!(
            engine.ingest(DuoInput::Hit(Hit {
                song_time: SongTime::from_frames(101),
                ..original
            })),
            Err(DuoError::ConflictingHit { .. })
        ));
        assert!(matches!(
            engine.ingest(DuoInput::Hit(Hit {
                epoch: SessionEpoch(6),
                seq: 2,
                ..original
            })),
            Err(DuoError::EpochMismatch { .. })
        ));
        assert!(matches!(
            engine.ingest(DuoInput::Hit(Hit { seq: 2, ..original })),
            Err(DuoError::ClosedHistory { .. })
        ));
        assert!(matches!(
            engine.ingest(DuoInput::Watermark {
                epoch: EPOCH,
                player: PlayerId::P1,
                through: SongTime::from_frames(99)
            }),
            Err(DuoError::WatermarkRegression { .. })
        ));
        assert!(matches!(
            engine.ingest(DuoInput::Hit(hit(PlayerId::P2, 2, i64::MAX))),
            Err(DuoError::TimeOverflow)
        ));
        for target in [&mut engine, &mut unchanged] {
            target
                .ingest(DuoInput::Hit(hit(PlayerId::P2, 2, 101)))
                .unwrap();
            close(target, 140);
        }
        assert_eq!(engine.events(), unchanged.events());
        assert_eq!(engine.resonance(), unchanged.resonance());
    }

    #[test]
    fn invalid_configuration_and_extreme_times_are_explicit() {
        let invalid = [
            DuoRules {
                precise_window_frames: -1,
                ..rules()
            },
            DuoRules {
                good_window_frames: 1,
                ..rules()
            },
            DuoRules {
                anchor_window_frames: 4,
                ..rules()
            },
            DuoRules {
                free_sync_window_frames: -1,
                ..rules()
            },
            DuoRules {
                anchor_sync_window_frames: -1,
                ..rules()
            },
            DuoRules {
                resonance_window_frames: 0,
                ..rules()
            },
            DuoRules {
                resonance_full_pairs: 0,
                ..rules()
            },
        ];
        for rules in invalid {
            assert!(matches!(
                DuoEngine::new(EPOCH, vec![], rules),
                Err(DuoError::InvalidRules(_))
            ));
        }
        assert!(matches!(
            DuoEngine::new(
                EPOCH,
                vec![],
                DuoRules {
                    anchor_window_frames: i64::MAX,
                    ..rules()
                }
            ),
            Err(DuoError::TimeOverflow)
        ));
        let anchor = Anchor {
            id: 1,
            song_time: SongTime::ZERO,
        };
        assert!(matches!(
            DuoEngine::new(EPOCH, vec![anchor, anchor], rules()),
            Err(DuoError::DuplicateAnchor(1))
        ));
        assert!(matches!(
            DuoEngine::new(
                EPOCH,
                vec![Anchor {
                    song_time: SongTime::from_frames(i64::MAX),
                    ..anchor
                }],
                rules()
            ),
            Err(DuoError::TimeOverflow)
        ));
        for at in [i64::MIN, i64::MAX] {
            let zero = DuoRules {
                precise_window_frames: 0,
                good_window_frames: 0,
                anchor_window_frames: 0,
                free_sync_window_frames: 0,
                anchor_sync_window_frames: 0,
                ..rules()
            };
            let mut engine = DuoEngine::new(EPOCH, vec![], zero).unwrap();
            for player in [PlayerId::P1, PlayerId::P2] {
                engine.ingest(DuoInput::Hit(hit(player, 1, at))).unwrap();
            }
            close(&mut engine, at);
            assert!(
                matches!(engine.events()[0], DuoEvent::FreeSync(FreeSyncEvent { midpoint, .. }) if midpoint.frames() == at)
            );
            assert_eq!(engine.resonance().matched_pairs, 1);
        }
    }
}
