# 品牌模块验证记录

日期：2026-10-02；独立程序直接编译交付的 `brand_intro.rs` 和 `brand_audio.rs`，没有通过修改共享模块注册来完成验证

## 结果

| 检查 | 结果 | 证据范围 |
|---|---|---|
| Cargo check / build | PASS | Bevy 0.19.1、Kira 0.12.5，独立 Cargo.lock |
| 单元测试 5 项 | PASS | 落点越界和去重、精确定稿值、落点位置与等比停靠、加载和暂停恢复、原创 PCM 边界 |
| Clippy 全目标 `-D warnings` | PASS | 实际两个品牌模块和验证程序 |
| rustfmt | PASS | 两个品牌模块和验证程序 |
| 1280×800 关键帧 | PASS | 12 帧，包含白字、碰撞、眼睛苏醒、定稿和停靠 |
| 1280×720 关键帧 | PASS | 12 帧，保持构图和完整可见 |
| 2560×1600 / scale 2 | PASS | 12 帧，逻辑视口与 1280×800 一致 |
| 30 fps 连续序列 | PASS | 135 帧，0–4.45 秒确定性采样 |
| 定稿无缝保持 | PASS | 3.80 和 4.00 秒 PNG 像素完全相同；材质参数单元测试与实体生命周期代码审查通过 |
| mask | PASS | 五张统一 3360×720，非透明像素 RGB 为白色，重复导出哈希一致 |
| 静态图标 | PASS | PNG 尺寸、透明边缘覆盖、两种 ICO 的尺寸目录；保留 image_gen 源 alpha，16px 外角存在缩放抗锯齿覆盖 |
| 来源哈希 | PASS | PROVENANCE.csv 的 18 个交付文件 SHA-256 与实物匹配 |
| 独立审查 | PASS | 已修正恢复首帧吞入暂停时长的问题；最终 ponytail-review 为 `Lean already. Ship.` |
| 生产启动、门控和音频接线 | NOT RUN | 共享文件由主线程统一接入 |
| 实际听感、设备延迟和平台图标 | NOT RUN | 无真实扬声器验收、Windows PE 或桌面环境安装验收 |

## 渲染证据

实际使用宿主 AMD Radeon RX 6650 XT、RADV Mesa 26.2.3-arch3.3、Vulkan；受限执行环境没有可用 GPU，获自动审批后在宿主运行成功，不能将初次环境不可用解释为 shader 故障

输出目录分别为 `/tmp/cocobeat-brand-1280x800`、`/tmp/cocobeat-brand-1280x720`、`/tmp/cocobeat-brand-dpi2` 和 `/tmp/cocobeat-brand-sequence`，日志为对应路径加 `.log`；这些临时证据不入 Git，可按 [验证程序说明](validation/README.md) 复现

1280×800 的 `frame_0008.png`（3.80 秒）和 `frame_0009.png`（4.00 秒）共享 SHA-256：`adb6211fb2122d16b57a53c229097987c40a5eb4a95939d378b81865fe29a6fe`

停靠可见边界在 1× 时为 `(39,26)–(273,72)`，2× 时为 `(78,53)–(547,144)`，像素边界取整符合等比缩放；未以截图代替真实窗口 resize 或输入验收

预览视频 `/tmp/cocobeat-brand-preview.mp4` 为 H.264、1280×720、30 fps，音频为 AAC、48 kHz 双声道，时长 4.50 秒；末帧保持运行时 4.45 秒姿态，多出的 0.05 秒来自固定帧率容器

视频 SHA-256：`53cef97192aaf84d512ea5eacb6817f4cc53da393392820f13d1a6c479eadb54`

预览音轨由验证程序输出的三段 PCM 在 0.90、2.10、3.35 秒合成，证明内容和时间安排可预览，不证明 Kira 实际播放或扬声器同步；主线程仍需验证消息消费与句柄生命周期

## 交接边界

完整 workspace 检查、现有游戏 Ready 到 Running 流程、真实焦点/设备门控、音频播放错误、Windows/Linux 四目标和发行图标由主线程接入后验证；本线程没有修改这些共享路径

资产参考图作者和许可未随附件提供，来源表按实际记录为未核实，不能把可编译或图形 PASS 解释成再分发许可已经确认
