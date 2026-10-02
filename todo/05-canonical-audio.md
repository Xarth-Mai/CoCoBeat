# 05 · 唯一标准音频路径

前置：本地体验通过。此时创建 cocobeat-media。

2026-10-02 的首轮前置隔离实验：`oxiaudio-encode 0.2.1` 与 `rusty_vorbis 0.1.1` 原始 API 未保持实际帧数，前者还出现严重越界幅度；`oxideav-vorbis 0.0.12` 原生封装与 rusty 显式 priming/mux 适配通过本次合成输入的有限检查，包含首尾脉冲和 64 秒双路完整回读，见 [实验记录与可复现工具](../docs/canonical-audio-probe.md)；当前未创建 media，Windows、真实音乐与听感尚未验收，以下正式准入任务保持未完成

同日后续实验：Rusty 十分钟帧数与资源限额通过，但近满幅 q5/q10 的右声道出现明显波形误差，当前版本不准入；OxideAV 同输入结构与波形观察更好，仍需完整准入；Symphonia 原生精确 seek 的头尾缺陷已复现，本批固定额外前滚的 100 窗口通过；OxiMedia 六例重采样 API 检查通过，发现帧数预估偏一帧，频响/抗混叠仍未测，完整方法与范围见上方实验记录

- [ ] Symphonia 导入；显式拒绝不支持格式并限制资源使用。
- [ ] 评估 OxiMedia audio 重采样：48 kHz、立体声，测量质量与性能。
- [ ] 验证最新纯 Rust Ogg Vorbis 编码器；报告提及的候选不视为已通过标准兼容验证。
- [ ] 重新解码最终 Ogg，检查采样率、声道、帧数、首尾瞬态、静默、clipping、seek 与长曲。
- [ ] MIR 的输入以最终回读音频为准，不能分析另一个时间原点的 PCM。
- [ ] SongPackage staging、对象哈希、版本头和原子 Ready 提交；损坏对象明确失败。

退出条件：Windows/Linux 编码回读与独立互操作证据通过。失败时阻止导入，在开发期替换实现，不引入 FFmpeg 或运行时备用编码路径。
