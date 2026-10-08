# 可审阅的 Anchor 提案

`propose-anchors` 根据已有 MusicAnalysis 的 onset 时间和置信度生成独立提案，记录每个 onset 的处理原因；`adopt-anchor-proposal` 重新核对完整来源与提案，只将明确选中的候选导出到新包。两步均为实验工具，当前生产 MIR 尚未提供通过标注校准的置信度，提案始终标记 `production_admission: "not_assessed"`

手工创作包目前没有 onset，运行提案命令会得到零候选和零 Anchor；命令不会从原手写谱面、能量或段落标签补出结果。人工增删移动已有 Anchor 使用 [内容编辑](editor.md)；独立人工标签通过 [`adopt-labeled-anchors`](independent-labels.md#明确采用为-anchor) 明确采用肯定精确点，保留 item_id 与原 frame，不把标签或 beat 候选分数转换成 onset confidence

## 生成与审阅

```sh
cargo run --locked -p cocobeat-lab -- propose-anchors PACKAGE MIN_CONFIDENCE MIN_GAP_FRAMES NEW_REPORT.json
```

两个策略参数必须显式传入，`MIN_CONFIDENCE` 是有限 f32 且位于 `(0, 1]`，`MIN_GAP_FRAMES` 是 `1..=28800000` 的整数音频帧；48,000 帧为一秒，没有默认参数或已校准的推荐值

先完整验证包和分析数据，再按置信度降序、时间升序选择候选；未知置信度或低于阈值的 onset 留空，恰好达到阈值可以参与。同已有选择的距离小于最小间隔时拒绝，恰好等于间隔允许；左右两侧都检查，拒绝证据取最近的已选 onset，等距时取较早者

选择结果按歌曲时间排序，ID 固定为原 onset 索引加一；证据保留原 onset 顺序及原始 `strength` / `confidence`，不量化或移动时间。相同合法输入和策略产生相同报告字节，不包含音频、机器路径、时间戳或随机值

报告包含 `report_version = 1`、`compiler_version = 1`、完整内容身份、三个对象哈希、实际帧数、分析与规则版本、策略、`anchors` 和 `evidence`；每条证据的 `decision.kind` 为以下之一

| 原因 | 附加字段 |
|---|---|
| `selected_by_experimental_policy` | `anchor_id` |
| `unknown_confidence` | 无 |
| `below_confidence` | 无 |
| `too_close` | `blocking_onset_index`、`distance_frames` |

输出报告最多 32 MiB，只创建新文件，父目录必须存在且位于源包外；通过符号链接父目录别名指向源包的路径也拒绝。成功 stdout 只给出来源身份、候选数、Anchor 数和准入状态，详细证据保存在报告中

## 明确选择与导出

审阅报告后创建选择 JSON，`onset_indices` 使用报告中的原始零基索引，`source_content_id` 使用报告的完整来源身份

```json
{
  "schema_version": 1,
  "source_content_id": "package-blake3:<完整64个小写十六进制字符的包哈希>",
  "onset_indices": []
}
```

`onset_indices` 是要保留的全部候选，顺序不限；空数组明确表示清空全部 Anchor，重复索引、越界索引或被策略拒绝的 onset 均报错。该操作替换整个 Anchor 列表，不与原手写 Anchor 合并

```sh
cargo run --locked -p cocobeat-lab -- adopt-anchor-proposal PACKAGE REPORT.json SELECTION.json NEW_PACKAGE
cargo run --locked -p cocobeat-lab -- verify-package NEW_PACKAGE
cargo run --locked -p cocobeat-game -- --package NEW_PACKAGE
```

选择文件最多 1 MiB，候选、证据和选择列表各不超过 100,000 项；未知字段、重复字段、非整数索引、非法版本和非法浮点值均拒绝。采用时完整验证源包身份，使用报告中记录的策略重新编译并比较全部报告内容，连同浮点正负零一起核对，然后只从重新计算的结果取出选择

导出复用 `media::export_anchors` 并再次检查完整源身份：新包保留原音频、analysis 字节和所有 SectionCue / rules，实际改变 Anchor 时重建 chart 与 manifest，完全无变化时保留原四对象字节及身份。输出目录与失败清理沿用 [内容编辑的导出契约](editor.md#原字节保留与内容身份)，原 Replay 继续只适用于原内容身份

纯编译器位于 [media/anchors.rs](../crates/cocobeat-media/src/anchors.rs)，仅消费分析、实际帧数和明确策略；[lab/anchors.rs](../tools/cocobeat-lab/src/anchors.rs) 负责报告、选择和包事务。提案不伪装成缺少 SectionCue 的完整 CompiledChart，现有 SongPackage / Replay v1 格式和 core 判定保持原契约
