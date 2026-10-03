# 07 · AnchorCompiler

生产准入前置：06 的分析与置信度证据；纯选择、审阅和采用软件可基于现有 MusicAnalysis 先行。编译器只消费分析数据，输出独立 AnchorProposal，再通过明确采用构成完整 CompiledChart，不接触 Bevy

- [x] 区分 HitAnchor 与 SectionCue，保留 AnchorEvidence：独立 AnchorProposal 保存全部 onset 决策，采用时保留源包 cue
- [ ] 依据标注确定置信度、最小间隔和密度策略，精度优先于召回。
- [x] 低置信度留空，记录接受与拒绝原因；未知置信度不补谱
- [x] 固定排序与 tie-break，同输入/配置产生相同输出
- [ ] 编译后时刻精确表示为 SongTime；人工试听与回放检查。

退出条件：稀疏 Anchor 可解释、可复现、可人工审阅，无隐藏补谱模式。

## 当前实施批次

2026-10-03 已实现纯提案编译器及 `propose-anchors` / `adopt-anchor-proposal` 两步 CLI，策略必须显式指定，报告绑定完整源包、保留每个 onset 的证据，并在人工选择采用前完整重编核对；原音频、分析字节与 SectionCue 保留，具体接口见 [Anchor 提案](../docs/anchors.md)

当前手工内容包没有 onset，因此得到空提案；非空流程使用明确构造的置信度测试选择机制，不将其当作音乐标注或 MIR 已校准输出。根据人工标注确定策略、生产自动分析、试听与可玩性仍未完成，完整 07 退出继续保留

41 项 media / lab 测试及 32 项真实包 CPU 检查通过，包含整数选择 oracle、全部证据比对、四种实际采用、源字节保留、错误拒绝与 core / Replay 逐项一致；详细命令、冻结身份和验收边界见 [验证策略](../docs/testing.md#anchor-提案审阅与明确采用)
