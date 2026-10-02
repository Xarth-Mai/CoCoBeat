# 品牌独立验证

从仓库根目录运行，独立 manifest 直接引用实际 `brand_intro.rs` 与 `brand_audio.rs`，固定 Bevy `0.19.1`、Kira `0.12.5`，不注册游戏系统、不创建窗口或音频设备

## 编译与窄测

```bash
CARGO_TARGET_DIR="$PWD/target" cargo test --offline --manifest-path assets/brand/validation/Cargo.toml
CARGO_TARGET_DIR="$PWD/target" cargo clippy --offline --manifest-path assets/brand/validation/Cargo.toml --all-targets -- -D warnings
CARGO_TARGET_DIR="$PWD/target" cargo build --offline --manifest-path assets/brand/validation/Cargo.toml
```

修改验证入口时，使用 `rustfmt --edition 2024 --config skip_children=true assets/brand/validation/main.rs`，避免格式化引用的其他线程文件

## 离屏渲染

需要可用的图形适配器，沙箱没有暴露 GPU 时在授权的宿主环境运行同一个二进制

```bash
target/debug/cocobeat-brand-validation --output /tmp/cocobeat-brand-1280x800 --size 1280x800 > /tmp/cocobeat-brand-1280x800.log 2>&1
target/debug/cocobeat-brand-validation --output /tmp/cocobeat-brand-1280x720 --size 1280x720 > /tmp/cocobeat-brand-1280x720.log 2>&1
target/debug/cocobeat-brand-validation --output /tmp/cocobeat-brand-dpi2 --size 2560x1600 --scale 2 > /tmp/cocobeat-brand-dpi2.log 2>&1
target/debug/cocobeat-brand-validation --output /tmp/cocobeat-brand-sequence --size 1280x720 --sequence > /tmp/cocobeat-brand-sequence.log 2>&1
```

`--size` 是输出物理像素，`--scale 2` 对应 2 倍 DPI，`2560x1600 --scale 2` 的逻辑视口为 `1280x800`

默认输出 12 个关键帧及 `frames.csv`；`--sequence` 输出从 0 至 4.45 秒的 135 张图片，用于 30 fps 预览；每个采样点冻结独立呈现时间，等待材质与布局稳定四帧，再等待截图落盘成功后推进，载入失败或超出帧数上限时以非零状态退出

每次运行同时导出 `pon-blue.wav`、`pon-pink.wav`、`whoom.wav`，均为 48 kHz 双声道 PCM16，时长依次为 0.28、0.32、0.62 秒；这些文件从实际音效生成函数获得，不涉及 Kira 播放设备验收

## 含音效的连续预览

```bash
ffmpeg -y -hide_banner -loglevel error \
  -framerate 30 -i /tmp/cocobeat-brand-sequence/frame_%04d.png \
  -i /tmp/cocobeat-brand-sequence/pon-blue.wav \
  -i /tmp/cocobeat-brand-sequence/pon-pink.wav \
  -i /tmp/cocobeat-brand-sequence/whoom.wav \
  -filter_complex '[1:a]adelay=900|900[a1];[2:a]adelay=2100|2100[a2];[3:a]adelay=3350|3350[a3];[a1][a2][a3]amix=inputs=3:normalize=0,apad,atrim=0:4.5[audio]' \
  -map 0:v -map '[audio]' -c:v libx264 -pix_fmt yuv420p -crf 18 \
  -c:a aac -b:a 192k -movflags +faststart /tmp/cocobeat-brand-preview.mp4
```

输出为 1280×720、30 fps、4.50 秒的 MP4，最后保持已完成的 4.45 秒姿态，音效按 0.90、2.10、3.35 秒定位；运行时仍由主线程消费 Bevy 消息并使用现有 Kira manager 播放

该程序验证真实 WGSL 管线与不同分辨率的渲染，不覆盖完整游戏输入门控、音频设备听感或发行平台集成
