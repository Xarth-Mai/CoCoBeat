# 05 · 唯一标准音频路径

前置：本地体验通过。此时创建 cocobeat-media。

- [ ] Symphonia 导入；显式拒绝不支持格式并限制资源使用。
- [ ] 评估 OxiMedia audio 重采样：48 kHz、立体声，测量质量与性能。
- [ ] 验证最新纯 Rust Ogg Vorbis 编码器；报告提及的候选不视为已通过标准兼容验证。
- [ ] 重新解码最终 Ogg，检查采样率、声道、帧数、首尾瞬态、静默、clipping、seek 与长曲。
- [ ] MIR 的输入以最终回读音频为准，不能分析另一个时间原点的 PCM。
- [ ] SongPackage staging、对象哈希、版本头和原子 Ready 提交；损坏对象明确失败。

退出条件：Windows/Linux 编码回读与独立互操作证据通过。失败时阻止导入，在开发期替换实现，不引入 FFmpeg 或运行时备用编码路径。
