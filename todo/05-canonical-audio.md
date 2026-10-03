# 05 · 唯一标准音频路径

前置：完整阶段退出仍需本地体验通过；2026-10-03 用户批准在设备与真人验收并行期间继续软件开发，现已创建 cocobeat-media，由 lab 消费有上限的源解码、固定 48 kHz 重采样与严格 canonical 读回

2026-10-02 的首轮前置隔离实验：`oxiaudio-encode 0.2.1` 与 `rusty_vorbis 0.1.1` 原始 API 未保持实际帧数，前者还出现严重越界幅度；`oxideav-vorbis 0.0.12` 原生封装与 rusty 显式 priming/mux 适配通过本次合成输入的有限检查，包含首尾脉冲和 64 秒双路完整回读，见 [实验记录与可复现工具](../docs/canonical-audio-probe.md)；当时未创建 media，Windows、真实音乐与听感尚未验收，以下正式准入任务按后续证据更新

同日后续实验：Rusty 十分钟帧数与资源限额通过，但近满幅 q5/q10 的右声道出现明显波形误差，当前版本不准入；OxideAV 同输入结构与波形观察更好，仍需完整准入；Symphonia 原生精确 seek 的头尾缺陷已复现，本批固定额外前滚的 100 窗口通过；OxiMedia 六例重采样 API 检查通过，发现帧数预估偏一帧，频响/抗混叠仍未测，完整方法与范围见上方实验记录

- [x] Symphonia 源导入；有上限的源解码与 lab 入口已完成，3 项窄测和正式 CLI 的 22 项格式/损坏/失败清理检查通过，64 秒原创音乐输出与独立 PCM16 转换逐字节一致；标准音频输出仍待后续接线
- [ ] 评估 OxiMedia audio 重采样：High 已接入 media/lab，合成质量、3 项行为测试、正式 CLI 19 项及独立 API 7 项通过，包含原创音乐位精确透传、十分钟流式摘要与 1 Hz 完整 flush 资源检查；跨平台执行与听感继续取证，见 [软件验证](../docs/testing.md#固定-48-khz-源重采样)
- [ ] 验证最新纯 Rust Ogg Vorbis 编码器；OxideAV 十分钟在 2 GiB 内存上限下失败；patched rusty q10 已完成四平台原生短样本测试与构建、原始 64 秒歌曲回归、Linux 十分钟静默及显式前滚 seek；后续三例有限超范围诊断通过，但原范围 guard 会拒绝合法重采样输出，继续确定编码数值域、曲库和听感准入；尚无生产准入编码器
- [x] 严格最终读回软件入口：`decode_canonical` 与 lab 的 `readback-canonical` 全量检查 Ogg Vorbis、48 kHz、恰好双声道、有限值、CRC、帧 0 连续性及调用方期望帧数；2 项新增行为测试与 21 项正式 CLI 检查通过，包含错误后半成品清理和已有输出保留，[原创样本](../testdata/synthetic/media-import/README.md) 与本地 `target/canonical-readback-20261003/validation-summary.json` 保留复现及证据
- [x] 最终音频对象准备：`prepare_canonical_audio` 与 lab 的 `prepare-audio` 有界复制最终 Ogg 至新 staging 目录，记录实际对象 BLAKE3/长度并严格读回副本；3 项新增行为测试覆盖身份、已有目录保护、上限和失败清理，11 项 media 测试通过；只准备音频对象，不创建虚构分析、谱面或 Ready
- [x] 固定全频带实验：冻结提交 `e1b3a26`，只比较长块 residue 扩带；19 例的 34 次编码与双路完整回读、4 次原 guard 拒绝及原 10 例字节复现通过；逐例保留全长和局部音质退步，见 [观察清单](../testdata/synthetic/canonical-audio-probe/fullband-observations-20261003.json)，正式 vendor/profile 与生产准入保持未变
- [ ] 最终编码闭环继续验收首尾瞬态、静默、clipping、seek、长曲和曲库听感，并在 Windows/Linux 各目标验证；严格读回入口通过不等于编码器准入或 SongPackage Ready
- [ ] 完整 MIR 的输入以最终回读音频为准；lab 的实际能量测量已从本次 staging 最终副本按帧 0 读取，自动 MIR 的编码回读对照仍待完成
- [x] 初始 SongPackage 四对象事务、BLAKE3、版本头和原子目录发布已由 lab 实际消费；staging 严格读回后从同一副本测量能量，组合带来源的手工 Anchor / 段落，完整复核后才发布，损坏与失败保留覆盖见 [契约](../docs/song-package.md)
- [ ] 将准入后的生产编码、完整 MIR / Anchor 输出和包事务接入游戏曲库 Ready；当前手工开发包不替代完整导入流程

退出条件：Windows/Linux 编码回读与独立互操作证据通过。失败时阻止导入，在开发期替换实现，不引入 FFmpeg 或运行时备用编码路径。
