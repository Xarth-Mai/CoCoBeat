# 品牌独立验证

从仓库根目录运行，独立 manifest 直接引用实际 `brand_intro.rs` 与 `brand_audio.rs`，按当前 lockfile 使用 Bevy `0.19.1`、Kira `0.12.5`，不注册游戏系统、不创建窗口或音频设备

## 编译与窄测

```bash
CARGO_TARGET_DIR="$PWD/target" cargo test --locked --offline --manifest-path assets/brand/validation/Cargo.toml
CARGO_TARGET_DIR="$PWD/target" cargo clippy --locked --offline --manifest-path assets/brand/validation/Cargo.toml --all-targets -- -D warnings
CARGO_TARGET_DIR="$PWD/target" cargo build --locked --offline --manifest-path assets/brand/validation/Cargo.toml
```

修改验证入口时，使用 `rustfmt --edition 2024 --config skip_children=true assets/brand/validation/main.rs`，避免格式化引用的其他线程文件

## 离屏渲染

需要可用的图形适配器，沙箱没有暴露 GPU 时在授权的宿主环境运行同一个二进制

```bash
target/debug/cocobeat-brand-validation --output /tmp/cocobeat-brand-1280x800 --size 1280x800 > /tmp/cocobeat-brand-1280x800.log 2>&1
target/debug/cocobeat-brand-validation --output /tmp/cocobeat-brand-1280x720 --size 1280x720 > /tmp/cocobeat-brand-1280x720.log 2>&1
target/debug/cocobeat-brand-validation --output /tmp/cocobeat-brand-dpi2 --size 2560x1600 --scale 2 > /tmp/cocobeat-brand-dpi2.log 2>&1
target/debug/cocobeat-brand-validation --output /tmp/cocobeat-brand-sequence --size 1280x720 --sequence > /tmp/cocobeat-brand-sequence.log 2>&1
target/debug/cocobeat-brand-validation --output /tmp/cocobeat-brand-menu-sequence --size 1280x720 --menu-sequence > /tmp/cocobeat-brand-menu-sequence.log 2>&1
```

`--size` 是输出物理像素，`--scale 2` 对应 2 倍 DPI，`2560x1600 --scale 2` 的逻辑视口为 `1280x800`

默认输出 41 个关键帧及 `frames.csv`，覆盖两次 C 顶部落点、各自眼睛出现、Beat 染完后对视的过冲与回弹、回正并停留，以及停靠移动中的拖拽、主矩形到位时仍在运动的字块、Co1 / Co2 / Beat 错峰达到位移与剪切峰值、6.797 / 6.836 秒两处字组间隙最窄时刻、反向回弹和精确归位；菜单采样覆盖从原稿视线回正、错峰眨眼、两轮换边领视的对视过冲与稳定、每轮视线回正、双眨和循环接缝

`--sequence` 输出从 0 至 7.20 秒的 217 张图片；`--menu-sequence` 输出从 0 至 31.20 秒的 937 张图片，包含启动动画及一个完整的 24 秒菜单循环，均用于 30 fps 预览

每个采样点冻结独立呈现时间，等待材质与布局稳定四帧，再等待截图落盘成功后推进，载入失败或超出帧数上限时以非零状态退出；启动时间在 `END` 封顶，之后只采样 `idle_seconds` 并启用 `idle_enabled`

阶段、结束时刻、循环周期及界面显露进度直接使用品牌模块的 `phase_at`、`END`、`IDLE_PERIOD` 和 `reveal_at`，球轨迹、晕染、眼睛和停靠姿态也由实际品牌模块计算

每次运行同时导出 `pon-blue.wav`、`pon-pink.wav`、`whoom.wav`，均为 48 kHz 双声道 PCM16，时长依次为 0.28、0.32、0.62 秒；这些文件从实际音效生成函数获得，不涉及 Kira 播放设备验收

## 含音效的连续预览

```bash
mkdir -p output/brand
ffmpeg -y -hide_banner -loglevel error \
  -framerate 30 -i /tmp/cocobeat-brand-sequence/frame_%04d.png \
  -i /tmp/cocobeat-brand-sequence/pon-blue.wav \
  -i /tmp/cocobeat-brand-sequence/pon-pink.wav \
  -i /tmp/cocobeat-brand-sequence/whoom.wav \
  -filter_complex '[1:a]adelay=900|900[a1];[2:a]adelay=2100|2100[a2];[3:a]adelay=3350|3350[a3];[a1][a2][a3]amix=inputs=3:normalize=0,apad,atrim=0:7.233333[audio]' \
  -map 0:v -map '[audio]' -c:v libx264 -pix_fmt yuv420p -crf 18 \
  -c:a aac -b:a 192k -movflags +faststart output/brand/cocobeat-intro-preview.mp4
```

输出为 1280×720、30 fps、约 7.23 秒的 MP4，最后保持已完成的 7.20 秒姿态，音效按 0.90、2.10、3.35 秒定位；运行时仍由主线程消费 Bevy 消息并使用现有 Kira manager 播放

完整启动与菜单预览使用同一编码命令，将输入目录改为 `/tmp/cocobeat-brand-menu-sequence`、音频截断改为 `atrim=0:31.233333`，输出改为 `output/brand/cocobeat-brand-preview.mp4`，得到约 31.23 秒的 MP4

从完整序列的第 216 帧开始取 720 帧，可以单独导出无音效的 24 秒菜单循环；第 936 帧是下一周期的同一原稿视线姿态，不重复放入循环视频

```bash
ffmpeg -y -hide_banner -loglevel error \
  -framerate 30 -start_number 216 -i /tmp/cocobeat-brand-menu-sequence/frame_%04d.png \
  -frames:v 720 -an -c:v libx264 -pix_fmt yuv420p -crf 18 \
  -movflags +faststart output/brand/cocobeat-menu-loop.mp4
```

观察眼神细节时，可在上述命令中加入 `-vf 'crop=280:90:20:8,scale=1120:360:flags=lanczos'`，输出改为 `output/brand/cocobeat-menu-detail.mp4`；这只放大真实停靠区域的像素，不改变运行时布局

停靠段可单独裁切并半速播放，用于观察三个字组在主体停止后的错峰收束：

```bash
ffmpeg -y -hide_banner -loglevel error \
  -framerate 15 -start_number 192 -i /tmp/cocobeat-brand-menu-sequence/frame_%04d.png \
  -frames:v 49 -vf 'crop=400:160:10:10,scale=1200:480:flags=lanczos' \
  -an -c:v libx264 -pix_fmt yuv420p -crf 18 \
  -movflags +faststart output/brand/cocobeat-dock-detail.mp4
```

该程序验证真实 WGSL 管线与不同分辨率的渲染，不覆盖完整游戏输入门控、音频设备听感或发行平台集成
