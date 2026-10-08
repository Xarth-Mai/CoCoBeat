# 05 · 唯一标准音频路径

前置：完整阶段退出仍需本地体验通过；2026-10-03 用户批准在设备与真人验收并行期间继续软件开发，现已创建 cocobeat-media，由 lab 消费有上限的源解码、固定 48 kHz 重采样与严格 canonical 读回

2026-10-02 的首轮前置隔离实验：`oxiaudio-encode 0.2.1` 与 `rusty_vorbis 0.1.1` 原始 API 未保持实际帧数，前者还出现严重越界幅度；`oxideav-vorbis 0.0.12` 原生封装与 rusty 显式 priming/mux 适配通过本次合成输入的有限检查，包含首尾脉冲和 64 秒双路完整回读，见 [实验记录与可复现工具](../docs/canonical-audio-probe.md)；当时未创建 media，Windows、真实音乐与听感尚未验收，以下正式准入任务按后续证据更新

同日后续实验：Rusty 十分钟帧数与资源限额通过，但近满幅 q5/q10 的右声道出现明显波形误差，当前版本不准入；OxideAV 同输入结构与波形观察更好，仍需完整准入；Symphonia 原生精确 seek 的头尾缺陷已复现，本批固定额外前滚的 100 窗口通过；OxiMedia 六例重采样 API 检查通过，发现帧数预估偏一帧，频响/抗混叠仍未测，完整方法与范围见上方实验记录

- [x] Symphonia 源导入；有上限的源解码与 lab 入口已完成，3 项窄测和正式 CLI 的 22 项格式/损坏/失败清理检查通过，64 秒原创音乐输出与独立 PCM16 转换逐字节一致；标准音频输出仍待后续接线
- [ ] 评估 OxiMedia audio 重采样：High 已接入 media/lab，合成质量、3 项行为测试、正式 CLI 19 项及独立 API 7 项通过，包含原创音乐位精确透传、十分钟流式摘要与 1 Hz 完整 flush 资源检查；跨平台执行与听感继续取证，见 [软件验证](../docs/testing.md#固定-48-khz-源重采样)
- [x] 静态内嵌原生编码软件入口：vorbis_rs q10、逐 block finite / ±4、真实 N、create_new 和返回错误清理已接入 media；39 项测试、14 项便携完整回读、38 项质量 / 拒绝控制和修补后 11 项 Sanitizer 通过，原失败保留，见 [方法与边界](../docs/native-vorbis.md)及[观察记录](../testdata/synthetic/native-vorbis-observations-20261007.json)
- [x] 验证唯一生产 Ogg Vorbis 编码器的软件入口；用户于 2026-10-07 批准静态内嵌成熟 C 库，已取得 vorbis_rs 本机及四目标原生软件证据，继续完整导入接线。旧纯 Rust 候选的 OxideAV 十分钟在 2 GiB 限制下失败，patched rusty q10 数值域 / 全频带仍有残余失真和局部退步，既有四平台及诊断证据保留；新路径本机和四目标所列编码 / 双回读软件控制已通过，生产源导入接线另取证，听感交用户真实验收
- [x] 严格最终读回软件入口：`decode_canonical` 与 lab 的 `readback-canonical` 全量检查 Ogg Vorbis、48 kHz、恰好双声道、有限值、CRC、帧 0 连续性及调用方期望帧数；2 项新增行为测试与 21 项正式 CLI 检查通过，包含错误后半成品清理和已有输出保留，[原创样本](../testdata/synthetic/media-import/README.md) 与本地 `target/canonical-readback-20261003/validation-summary.json` 保留复现及证据
- [x] 最终音频对象准备：`prepare_canonical_audio` 与 lab 的 `prepare-audio` 有界复制最终 Ogg 至新 staging 目录，记录实际对象 BLAKE3/长度并严格读回副本；3 项新增行为测试覆盖身份、已有目录保护、上限和失败清理，11 项 media 测试通过；只准备音频对象，不创建虚构分析、谱面或 Ready
- [x] 固定全频带实验：冻结提交 `e1b3a26`，只比较长块 residue 扩带；19 例的 34 次编码与双路完整回读、4 次原 guard 拒绝及原 10 例字节复现通过；逐例保留全长和局部音质退步，见 [观察清单](../testdata/synthetic/canonical-audio-probe/fullband-observations-20261003.json)，正式 vendor/profile 与生产准入保持未变
- [x] 固定数值域与全频带组合：四个既有源各编码一次并双路完整回读，通过实际核系数与阶段预算核对；保留残余失真、局部退步及条件范围，见 [诊断](../tools/canonical-audio-probe/numeric-fullband/README.md)，正式 guard/profile 未改
- [x] 本机 canonical q10 定位窗口软件控制：8 个固定对象的 695 次明确前滚窗口与完整回读匹配，32 次越界拒绝通过；原始 Accurate 的 127 次 API 失败保留，实际 delay 128 与前滚 1024 分别记录，[观察清单](../testdata/synthetic/native-vorbis-seek-20261007.json)。游戏完整 PCM 路径未改，四平台 seek 与生产流式解码另验
- [ ] 最终编码闭环继续验收首尾瞬态、静默、clipping、seek、长曲和曲库听感，并在 Windows/Linux 各目标验证；严格读回入口通过不等于编码器准入或 SongPackage Ready
- [ ] 完整 MIR 的输入以最终回读音频为准；lab 的实际能量测量已从本次 staging 最终副本按帧 0 读取，自动 MIR 的编码回读对照仍待完成
- [x] 初始 SongPackage 四对象事务、BLAKE3、版本头和原子目录发布已由 lab 实际消费；staging 严格读回后从同一副本测量能量，组合带来源的手工 Anchor / 段落，完整复核后才发布，损坏与失败保留覆盖见 [契约](../docs/song-package.md)
- [x] 手工包运行时入口：`--package DIR` 从同一份校验字节读取完整 PCM，Kira 使用共享 PCM 重播，Session / HUD 使用真实长度与 Anchor，Replay 绑定完整包身份；保留完整开场和 Ready 的独立确认，完整曲库和内容编译仍按后续任务推进
- [x] 手工段落运行时表现：包内 SectionCue 驱动下一提示字幕和六秒预告门，实际歌曲游标控制暂停、重启与结束，字体回退覆盖现有 Noto 支持的混合脚本；103 项 runtime 测试、13 项 CLI 和 10 张静态 GPU 图通过，不代表自动舞台编译完成
- [x] 有界原始音频经唯一生产编码器和手工 authoring 导入四对象，真实 N / 能量 / 来源写入包并由生产游戏加载；10 条成功 CLI、5 条预期拒绝及两组原生 loader 图通过，见[源导入](../docs/source-import.md)
- [x] Ready 生产曲库选择已制作的四对象歌曲包，后台完整验证后才替换歌曲与 PCM；取消和三类拒绝保留旧包 / Replay，六项原生软件检查与 42 张指定范围截图通过，旧小窗口视觉失败保留，见[曲库验证](../docs/testing.md#生产曲库选择与小窗口反馈)
- [x] 游戏手工源导入 CLI：`--import-authored SOURCE AUTHORING NEW_PACKAGE` 复用 media 与 lab 的共享包事务，完成校验后进入完整开场与 Ready；独立开始确认和运行验证见[源导入](../docs/source-import.md)
- [x] 游戏 Ready 手工源导入入口：正常启动后从曲库进入 `imports` 配对音频与 authoring，确认新目标后复用单 worker、唯一编码器、四对象事务和完整 loader，成功后停 Ready 等新确认；返回仅放弃自动选择，后台完成 / 错误与关闭 join 保留，见[操作与边界](../docs/source-import.md#正常启动的-ready-导入)
- [ ] 将完整 MIR / Anchor 输出和包事务接入游戏曲库 Ready；手工配对导入不替代合格自动分析、Anchor 策略及人工标签
- [x] 完整内容软件路线：正式导入→未标注 Anchor 推断 / 明确采用→结构与重复分析尝试→四对象复核 / Stage→正式 Library Ready / 新确认播放、暂停、切歌、拒绝和取消；18 条 CPU / CLI 与独立原生窗口通过，合法无支持以 Unsupported 保留完整原 facts，原严格候选拒绝与启动前预算 FAIL 保留，见[路线观察](../testdata/synthetic/full-content-route-observations-20261009.json)。本项不关闭上一项的音乐质量 / 自动 Anchor 策略或设备验收

退出条件：Windows/Linux 编码回读与独立互操作证据通过。失败时阻止导入，在开发期替换实现，不引入 FFmpeg 或运行时备用编码路径。

2026-10-07 · 原生编码四目标软件补验：固定源码 8a31a1d 在 Windows / Linux 两架构各执行 14 项控制成功，Linux 各 39、Windows 各 37 项 media 测试通过；Windows 的两项 Unix 专用检查未编译。四份官方归档摘要、源码与实际输出独立核验通过，见[平台观察记录](../testdata/synthetic/native-vorbis-platforms-20261007.json)。真实音乐、听感、平台 seek 与游戏发行包仍按各自证据验收
