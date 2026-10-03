# Replay 规则诊断

`inspect-replay` 读取真实歌曲包与 Replay v1，用现有 `Replay::replay` 和同一个 DuoEngine 重放，再把原始输入、水位、已确认判定及配对保存为 JSONL，供人工检查和后续时间线界面使用

```sh
cargo run --locked -p cocobeat-lab -- inspect-replay PACKAGE REPLAY.json NEW_REPORT.jsonl
```

命令先完整验证歌曲包，只接受当前 `duo-watermark-v1`，再有界解码 Replay、检查所有 Hit 的歌曲范围并校验完整内容 / 规则身份；相同音频但不同谱面不能沿用旧 Replay。原 `build_id` 保留为来源记录，不作为兼容门槛，报告不改绑录制或重新解释歌曲时间

## 可检查的事实

每行一个 JSON object，以换行结束；顺序固定为 header、原文件顺序的全部 facts、core 顺序的全部已确认 events、summary。同一包和 Replay 在相同报告版本与规则下生成相同字节

| `type` | 内容 |
|---|---|
| `header` | `format = "CoCoBeat Replay Diagnostic"`、`report_version = 1`、完整内容 / 规则 / 原构建身份、epoch、实际歌曲帧数、Anchor / fact 数、实际规则参数与确认延迟帧 |
| `hit` | 原 `fact_index`、epoch、玩家、seq 与整数 `song_time_frames` |
| `watermark` | 原 `fact_index`、epoch、玩家与整数 `through_frames` |
| `anchor_judged` | `event_index`、Anchor ID / 原帧、玩家、等级、偏差及原 Hit 关联 |
| `free_sync` | 双方原 Hit 的 seq / fact_index / 歌曲帧，以及 core 的偏差和中点帧 |
| `anchor_sync` | Anchor ID / 原帧、双方完整判定与各自 Hit 关联、相对偏差 |
| `summary` | 原始事实与已确认事件数、双方 Hit 数、已确认 / 待确认 Anchor 数、两类 Sync 数、双方最后水位和完整最终 Resonance |

玩家为 `1` / `2`，`fact_index` 与 `event_index` 均从 `1` 开始；缺失水位、未命中的 Hit 序号 / 时刻使用 `null`，不填零。等级为 `precise`、`good`、`late_or_early` 或 `miss`，原始整数完整保留，不通过浮点秒回写

事件里的 Hit 关联来自已验证事实的 `(player, seq)` 查找，偏差、等级和配对直接取 core 结果；事件序号不表示它由同序号 fact 触发，也不编造消费时刻或壁钟时间

没有双方水位就不会补出尚未确认的 Miss；负预滚水位、歌曲 EOF 后的合法收尾水位仍如实保留。报告不自动追加结束水位，待确认 Anchor 为零也不表示整段共享历史或会话已经完成

## 限额和文件保护

Replay 输入沿用 v1 的 20 MiB、160,000 facts 和每个身份最多 256 UTF-8 字节限制，要求普通文件；未知版本、非法字段、损坏内容或 core 无法接受的历史明确失败。Hit 必须位于 `[0, canonical_frames)`，水位由原 core 的单调性和历史规则校验，不套用 Hit 的曲内范围

输入范围错误和 core 语义错误包含原事实序号，例如 `Replay fact 2:`；所有 Hit 范围先检查，再执行身份与 core 重放，因此错误序号不承诺跨验证阶段寻找最早错误

输出最多 128 MiB、540,002 行，每行最多 8 KiB，均包含换行；逐行写入，超限失败而不截断事实。报告只创建新文件，父目录必须存在且位于源包外，包含符号链接父目录别名检查；已有文件、目录、符号链接及源 Replay 本身不覆盖

失败时只清理本调用新建的报告，清理失败会一并报错，原包与 Replay 保持不变；普通新文件写入不提供额外原子发布保证。成功 stdout 输出报告版本、内容身份及 fact / event 数，详细事实留在报告中

## 与计时和编辑的关系

Replay v1 没有保存设备时间、软件观察 / 消费时间、音频回调位置或时钟不确定性，本报告不推算这些字段或把歌曲时间差当物理延迟。Session 的另存 CSV 尚缺完整内容 / epoch 关联，本入口不按同名文件自动合并 CSV

[Anchor 编辑](editor.md)的无变化导出保留原身份，可继续生成相同诊断；真实改谱后的新包需要匹配自身身份的新录制，原录制不会自动改绑。JSONL 是开发诊断入口，波形时间线、Replay 图形界面和真实设备计时仍按各自任务实现
