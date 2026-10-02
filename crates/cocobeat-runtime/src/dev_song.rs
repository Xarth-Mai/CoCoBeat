//! Original CC0-1.0 development song; generator source remains MPL-2.0
//! PCM frames are deterministic integer synthesis, not canonical Ogg import output

pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAMES: u32 = 3_072_000;
pub const BPM: u32 = 120;
pub const ANCHOR_FRAMES: [i64; 7] = [
    1_248_000, 1_440_000, 1_632_000, 1_824_000, 2_400_000, 2_592_000, 2_784_000,
];
const BEAT: u32 = 24_000;

pub fn anchors() -> Vec<cocobeat_schema::Anchor> {
    ANCHOR_FRAMES
        .iter()
        .enumerate()
        .map(|(index, &frame)| cocobeat_schema::Anchor {
            id: index as u64 + 1,
            song_time: cocobeat_schema::SongTime::from_frames(frame),
        })
        .collect()
}

/// PCM16 conversion uses a power-of-two divisor and is exact in f32
pub fn samples() -> Vec<[f32; 2]> {
    (0..FRAMES)
        .map(|frame| sample(frame).map(|sample| f32::from(sample) / 32_768.0))
        .collect()
}

// Integer synthesis keeps the development source byte-identical across hosts
fn triangle(frame: u32, milli_hz: u32) -> i64 {
    let phase = (u64::from(frame) * u64::from(milli_hz) % 48_000_000) as i64;
    let quarter = 12_000_000;
    let height = match phase / quarter {
        0 => phase,
        1 | 2 => 2 * quarter - phase,
        _ => phase - 4 * quarter,
    };
    height * 32_767 / quarter
}

fn tone(frame: u32, milli_hz: u32, length: u32, amplitude: i64) -> i64 {
    if frame >= length {
        return 0;
    }
    let attack = i64::from(frame.min(240));
    let decay = i64::from(length - frame);
    triangle(frame, milli_hz) * amplitude * attack * decay / (32_767 * 240 * i64::from(length))
}

pub fn sample(frame: u32) -> [i16; 2] {
    if frame >= FRAMES || (1_920_000..2_304_000).contains(&frame) {
        return [0, 0];
    }
    let beat = frame / BEAT;
    let within = frame % BEAT;
    let bar = beat / 4;
    // Original eight-note pentatonic motif over four bass roots, not imported music
    let melody = [
        261_626, 329_628, 391_995, 440_000, 391_995, 329_628, 293_665, 261_626,
    ];
    let bass = [65_406, 87_307, 110_000, 98_000][(bar % 4) as usize];
    let kick = tone(within, 55_000, 7_200, 5_000);
    let low = tone(frame % (BEAT * 2), bass, BEAT * 2, 3_000);
    let lead = if (16..80).contains(&beat) || (96..120).contains(&beat) {
        tone(within, melody[(beat % 8) as usize], 18_000, 3_200)
    } else {
        tone(frame % (BEAT * 4), 261_626, BEAT * 4, 1_600)
    };
    let half_beat = frame % (BEAT / 2);
    let hat = if half_beat < 1_200 && (16..120).contains(&beat) {
        let noise = frame.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
        let noise = (noise ^ (noise >> 16)) & 0xffff;
        (i64::from(noise) - 32_768) * i64::from(1_200 - half_beat) / (32_768 * 3)
    } else {
        0
    };
    let fade = i64::from((FRAMES - frame).min(192_000));
    let left = (kick + low + lead + hat) * fade / 192_000;
    let right = (kick + low + lead * 3 / 4 - hat) * fade / 192_000;
    [left as i16, right as i16]
}
