//! 与渲染、设备及网络无关的双人规则事实

use crate::{SessionEpoch, SongTime};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum PlayerId {
    P1,
    P2,
}

impl PlayerId {
    pub const fn index(self) -> usize {
        match self {
            Self::P1 => 0,
            Self::P2 => 1,
        }
    }
}

/// seq 在同一 epoch 和玩家内唯一，交付顺序不改变输入时间
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Hit {
    pub epoch: SessionEpoch,
    pub player: PlayerId,
    pub seq: u64,
    pub song_time: SongTime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Anchor {
    pub id: u64,
    pub song_time: SongTime,
}

/// 所有时间窗口均包含边界，默认值是待真人实验调整的初始参数
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DuoRules {
    pub precise_window_frames: i64,
    pub good_window_frames: i64,
    pub anchor_window_frames: i64,
    pub free_sync_window_frames: i64,
    pub anchor_sync_window_frames: i64,
    pub resonance_window_frames: i64,
    pub resonance_full_pairs: u32,
}

impl Default for DuoRules {
    fn default() -> Self {
        Self {
            precise_window_frames: 2_160,
            good_window_frames: 4_320,
            anchor_window_frames: 8_640,
            free_sync_window_frames: 3_600,
            anchor_sync_window_frames: 3_600,
            resonance_window_frames: 192_000,
            resonance_full_pairs: 8,
        }
    }
}

impl DuoRules {
    /// 为候选输入及可能使用它的 Anchor 留足确认时间
    pub fn confirmation_delay_frames(self) -> Option<i64> {
        self.anchor_window_frames
            .checked_mul(2)?
            .checked_add(self.free_sync_window_frames)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DuoInput {
    Hit(Hit),
    /// 此玩家的所有 song_time <= through 输入均已交付
    Watermark {
        epoch: SessionEpoch,
        player: PlayerId,
        through: SongTime,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnchorGrade {
    Precise,
    Good,
    LateOrEarly,
    Miss,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnchorJudgement {
    pub player: PlayerId,
    pub anchor_id: u64,
    pub input_seq: Option<u64>,
    pub offset_frames: Option<i64>,
    pub grade: AnchorGrade,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FreeSyncEvent {
    pub p1_input: u64,
    pub p2_input: u64,
    pub delta_frames: u64,
    /// 两次输入的中点，半帧向负无穷取整
    pub midpoint: SongTime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnchorSyncEvent {
    pub anchor_id: u64,
    pub p1: AnchorJudgement,
    pub p2: AnchorJudgement,
    pub relative_delta_frames: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DuoEvent {
    AnchorJudged(AnchorJudgement),
    FreeSync(FreeSyncEvent),
    AnchorSync(AnchorSyncEvent),
}

/// 查询共同已确认历史的滚动窗口；千分比限制在 0..=1000
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResonanceState {
    pub through: Option<SongTime>,
    pub inputs: [u64; 2],
    pub matched_pairs: u64,
    pub mutual_match_per_mille: u16,
    pub activity_per_mille: u16,
    pub level_per_mille: u16,
}
