# Replay 规则诊断

`inspect-replay` 与 `workbench-replay` 读取真实歌曲包与 Replay v1 / v2，共用完整校验和唯一 `Replay::replay` / DuoEngine 重放；前者保存 JSONL，后者在原生波形窗口中只读查看原始输入、水位、已确认判定及配对

```sh
cargo run --locked -p cocobeat-lab -- inspect-replay PACKAGE REPLAY.json NEW_REPORT.jsonl
cargo run --locked -p cocobeat-lab -- workbench-replay PACKAGE REPLAY.json [--locale CODE]
```

命令先完整验证歌曲包，只接受当前 `duo-watermark-v1`，再有界解码 Replay、检查所有 Hit 的歌曲范围并校验完整内容 / 规则身份；相同音频但不同谱面不能沿用旧 Replay。原 `build_id` 保留为来源记录，不作为兼容门槛，报告不改绑录制或重新解释歌曲时间

## 图形查看

图形入口复用[原生工作台](editor.md#只读-replay-工作台)的波形、单一主控和虚拟列表，P1 / P2 Hit 分别使用青色 / 橙色，列表逐条保留原 facts 与已确认 events 的顺序。选中 Hit 定位原始整数帧，Anchor 判定定位原 Anchor，Free Sync 定位 core 中点并同时高亮双方原 Hit；详情保留各自的 fact_index、seq、歌曲帧、等级与偏差，不把事件索引解释为触发事实索引

负预滚水位和 EOF 后的水位在行、光标及详情中保持原值，波形视口仍限于实际歌曲；空或单方历史保持待确认状态，不追加水位或补出尚未确认的 Miss。相同帧的多条记录可分别选择，详情按选中记录、header、summary 排列，640×480 使用列表 / 详情页切换和可滚动长文本

该入口只浏览，隐藏编辑、撤销重做和导出控件，并在输入路径阻止修改；原包、Replay 与游戏配置保持不变；后续共用工作台的试听控件，音频源游标与原事实光标分开，设备计时不从歌曲帧推算

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

Replay v1 / v2 输入沿用 20 MiB、160,000 facts 和每个身份最多 256 UTF-8 字节限制，要求普通文件；未知版本、非法字段、损坏内容或 core 无法接受的历史明确失败。Hit 必须位于 `[0, canonical_frames)`，水位由原 core 的单调性和历史规则校验，不套用 Hit 的曲内范围

输入范围错误和 core 语义错误包含原事实序号，例如 `Replay fact 2:`；所有 Hit 范围先检查，再执行身份与 core 重放，因此错误序号不承诺跨验证阶段寻找最早错误

输出最多 128 MiB、540,002 行，每行最多 8 KiB，均包含换行；逐行写入，超限失败而不截断事实。报告只创建新文件，父目录必须存在且位于源包外，包含符号链接父目录别名检查；已有文件、目录、符号链接及源 Replay 本身不覆盖

失败时只清理本调用新建的报告，清理失败会一并报错，原包与 Replay 保持不变；普通新文件写入不提供额外原子发布保证。成功 stdout 输出报告版本、内容身份及 fact / event 数，详细事实留在报告中

## 显式本机计时关联

```sh
cargo run --locked -p cocobeat-lab -- inspect-replay PACKAGE REPLAY.json NEW_REPORT.jsonl --timing SIDECAR.timing.json
cargo run --locked -p cocobeat-lab -- workbench-replay PACKAGE REPLAY.json --timing SIDECAR.timing.json [--locale CODE]
```

`--timing` 只消费明确指定的一份[本机软件计时 sidecar](timing.md#本机-replay-软件计时)，在图形命令中置于可选 `--locale` 前；其他工作台不接受此选项。未传时维持原 v1 / v2 报告、stdout 和 GUI 内容，不寻找同名 CSV / JSON，不按文件名、Hit 数或接近的歌曲时间猜绑定

同一次打开的普通 Replay 有界原字节用于 decode、原包 / Hit 范围 / 唯一 core 校验及 sidecar raw BLAKE3 绑定，不重开文件计算身份，也不对重新编码的 JSON 猜原 hash；仅改变空白字节也不再是同一精确 recording。sidecar 的 content / rules / build / 明确 Stage / epoch / canonical_frames 必须与该 Replay 和源包相等；原 build_id 不作跨版本 core 兼容门槛，但 sidecar 必须保留它绑定的这份原身份，旧 v1 的 Stage 保持明确 `null`

共享校验要求 Capture 按原事实顺序覆盖声明本地席位的全部 Hit，1-based fact_index 指向完全相同的 player / seq / song frame，不重复、不遗漏、不给远端补软件时间；mapping anchor 不晚于原 observed 时刻，消费时刻不得早于观察，source / callback 的各自序列与 nullable 前驱保持。该结构校验不把软件时间变成经过测量的物理精度

显式关联使用 `report_version = 3`，header 的 `local_timing` 保存 raw Replay hash、原 Stage nullable、clock 配置、本地席位、Capture 数与稀疏历史覆盖；Hit 和 AnchorJudged 的 `local_timing`、Sync 双方各自的 `p1 / p2.local_timing` 均沿原 Hit fact_index 查询，Watermark、core 判定与 summary 不改写。匹配 Capture 保留原字段和软件消息消费等待；远端未录制为 `not_recorded`，未关联 Hit 的 Miss 为 `no_hit`，不补造数值

既有可滚动详情展示本机记录和选中 Hit 的真实缺失状态，保留原 waveform、事实光标、试听游标与 read-only 控制；关联 Capture 前后最近的实际 AudioRead 按本 origin 的 read 时刻显示，缺失使用 `null`，不插值、不重新映射 Hit，也不说该 source / callback 就是 Hit 当时或同一次 backend callback。消息观察、消费等待及独立历史快照不能宣称物理输入或声学延迟

sidecar 输入要求普通有界文件，128 MiB、12,000 AudioRead 和最多 160,000 Capture 的限制与采集端相同；未知字段 / 版本、错误字节身份或 fact join 在创建报告之前拒绝。原报告的源包外 create_new、8 KiB 行限额、128 MiB 总限额和失败清理保护保持，不覆盖原 Replay / sidecar / 包对象；完整 sidecar 音频历史不展开为无界 JSONL 单行

共享、runtime、lab 与边界检查通过，实际 Lab 完成 32 条 CLI 调用，原生工作台副本完成真实记录选择与详情滚动，三张图的指定范围可读性目检通过；13 个语言变体的五个新文案字形覆盖通过，完整记录和首次 QA 焦点失败见[计时观察](../testdata/synthetic/timing-sidecar-observations-20261008.json)。字体覆盖不代表其他窗口 / 母语校对，物理设备、DAC / 扬声器与真人体验继续分别验收

## 与计时和编辑的关系

Replay v1 / v2 的事实文档不增加设备时间、软件观察 / 消费时间或音频历史，默认诊断不推算这些字段或把歌曲时间差当物理延迟；显式 sidecar 保持独立来源和精确原字节关联。既有另存 CSV 保留生成行为和历史文件，但缺完整字节身份，本入口不自动升级、猜测或合并它

Replay v1 没有视觉或着色器版本记录，缺失版本始终保留为未知，原 `build_id` 不等于视觉兼容证明；Replay v2 明确记录 `stage_compiler_version = 1 / 2`，只绑定确定性几何编译，不记录 shader、呈现设置或设备表现

`cocobeat-lab inspect-replay-stage PACKAGE REPLAY FRAME` 先沿用同一完整包、身份、整数事实范围与 core 校验，再按明确记录的版本重建 StagePlan 并输出整数采样；v1 缺字段明确失败，不猜测历史版本。v1 图形诊断与 JSONL 的原字节保持，v2 JSONL 使用 `report_version = 2` 并在 header 增加 `stage_compiler_version`

runtime 的 `--replay` 保持退出式 core 校验；独立 `--watch-replay` 接通下面的原生只读观看，几何版本使用明确记录，历史 shader、网络到达时间和设备计时仍没有记录

[Anchor 编辑](editor.md)的无变化导出保留原身份，可继续生成相同诊断；真实改谱后的新包需要匹配自身身份的新录制，原录制不会自动改绑。本机软件计时关联按上面的显式 sidecar 入口推进；试听校准、物理设备计时和真实输入验收继续独立推进

## 来源出版漂移观察

显式 `--timing SIDECAR` 的既有 CLI 与工作台共同消费原 `audio_history`，在 `local_timing.source_drift` 报告原出版时间区间与最近帧软件游标的速率区间；每端点保留一帧余量，差值留 ±2 帧，ppm 使用整数有向舍入。只统计实际记录的 Running、同 generation / source_id 且推进的来源；Pause、缺来源、过期、读缺口或身份变化分段，重复出版不刷新年龄

有可测段时仅保留 elapsed 下界最长的一段、原首尾出版和原 audio_history 索引，完整原记录仍在侧车；没有可测段输出 `unknown` 和 null。保留 8 KiB 行上限，稀疏读之间未观察的转换未知，结果不表示 DAC 精度或长期漂移已校正

2026-10-08 的 [软件记录](../testdata/synthetic/calibration-drift-observations-20261008.json)包含两份原计时侧车的实际 CLI、无 timing 的旧报告完整字节保持、null 来源 unknown 和错误绑定拒绝；所有原 timing 字段与行保持，独立有理数四角核对区间。最初 null 侧车配错 Replay 的 QA FAIL 保留，补验仅纠正原绑定；新硬件采样、GUI 目检与双端漂移纠正另验

## 原生只读 Replay 观看

```sh
cargo run --locked -p cocobeat-game -- --package PACKAGE --watch-replay REPLAY.json
```

观看前完整验证四对象、实际 PCM、完整内容 / 规则身份、Hit 范围和同一 core 的原始历史；只接受明确记录的 Stage 1 / 2，以 analysis 段落区间和实际长度编译该版本。缺失舞台版本的旧录制仍有效用于 core 诊断，观看明确报错，不猜测历史几何

每次进程启动完整播放品牌开场，然后保持 Ready，明确的新确认才启动歌曲；观看复用现有真实 Kira、设置、暂停和菜单主控，键盘与手柄可接管菜单。界面只显示观看标记与原事实进度，隐藏玩家加入 / Hit 绑定和保存录制；实况 Hit 和直接保存快捷键无效，原包 / Replay 和录制目录不写入

事实可见性由已观察到的 Kira source position 决定，不使用重复 callback 的歌曲游标外推揭示更多事实；保留原文件顺序，位于前面的未来水位可阻挡后面的较早另一位玩家 Hit。负水位在零帧可见，超出 EOF 的原合法水位只在自然结束时送入 core，值保持原样

自然 EOF 只消费剩余原事实，没有追加结束水位、假命中或补 Miss；只有单方水位的录制仍保持未确认 Anchor。重新开始恢复原 epoch、空消费游标和新引擎，使用同一 PCM；暂停由 callback acknowledgment 确认，返回菜单停止音乐并等待下一次新确认

一次可见的多个同玩家 Hit 合并为一个呈现音效与角色脉冲，原始事实和 core 事件全部保留；这是一种按歌曲帧观看的表示，不重造录制时的网络接收时刻。舞台版本绑定几何，当前 shader / 字体 / 画质不会被当作历史渲染器

## 软件验证

2026-10-07 本批 lab / net / runtime / xtask 共 169 项测试、Clippy 和构建通过；正式 `inspect-replay` 对完整网络录制与单事实前缀生成的报告及 stdout 均与旧版逐字节一致，具体来源与命令见[本批观察清单](../testdata/synthetic/session-diagnostics-observations-20261007.json)

受控 GUI 验证使用冻结的五个生产源码文件副本，保持输入、布局与重放逻辑原样，只在副本附加窗口尺寸、合成 `KeyboardInput` 驱动和 Bevy 原生截图系统，再链接正式 Cargo JSON 中的精确 extern；[复现工具](../tools/replay-workbench-check/README.md)记录源 / helper / 依赖 SHA-256、注入差异、命令与退出码

1280×800 和 640×480 各退出 0、逐条核对 76 条记录并保存 5 张 PNG，合计 10 张图已目检；负水位 -1632、Free Sync 双 Hit 与中点 26462、列表末尾和详情顶部 / 底部均有状态断言，原包、Replay 与期望报告字节保持不变。该证据覆盖合成输入和原生 GPU 渲染，物理输入、扬声器、真人体验及历史视觉复现为 NOT RUN

原生观看软件证据：133 项 runtime 测试与最终标题窄测通过，四个实际窗口 case 分别覆盖两个版本 full / partial，最终两尺寸英文及德语标题静态图另行取证；旧尺寸与 GPU 失败保留，实际播放和最终文本不混同，详见[原生观看验证](testing.md#原生只读视觉-replay)与[持久记录](../testdata/synthetic/visual-replay-observations-20261007.json)
