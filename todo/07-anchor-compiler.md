# 07 · AnchorCompiler

前置：06。只消费 MusicAnalysis，输出自有 AnchorMap / CompiledChart，不接触 Bevy。

- [ ] 区分 HitAnchor 与 SectionCue，保留 AnchorEvidence。
- [ ] 依据标注确定置信度、最小间隔和密度策略，精度优先于召回。
- [ ] 低置信度留空，记录接受与拒绝原因。
- [ ] 固定排序与 tie-break，同输入/配置产生相同输出。
- [ ] 编译后时刻精确表示为 SongTime；人工试听与回放检查。

退出条件：稀疏 Anchor 可解释、可复现、可人工审阅，无隐藏补谱模式。
