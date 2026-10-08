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

## 显式实验校准与密度策略

`train-anchor-calibration` 只用训练标签拟合固定 score bin 的 Beta(1,1) 估计，输出训练计数与各预声明策略的计数；`evaluate-anchor-calibration` 先核对训练结果和冻结 Choice，再读取 heldout 标签，报告所选策略计数、已知分母与 Brier。结果始终 `quality_status: "UNASSESSED"`、`production_admission: false`，不自动选择策略或准入曲库

Input 使用严格 schema 1，指定 `scope: "experimental_anchor_calibration"`、`method: "fixed_bin_beta11"`、`score_definition: "original_hfc_normalized_strength_f32_bits"`、固定自动 profile 与声道、2–9 个递增有限 f32 `edge_bits`、明确支持数及 1–8 个策略；每个策略明确 confidence、gap、density window 和 capacity。训练与评估各 1–8 组，绑定原 Source、analysis / native summary / 标签原字节 hash、来源 group、完整 `[0,N)` 范围和至多 1024 条 onset index / frame / scorebits / label item_id 精确点映射；两 split 的音频身份与 group 分离，各有肯定与否定判断，声明本身不证明真人独立审阅

```sh
cocobeat-lab train-anchor-calibration INPUT.json NEW_TRAINING.json
cocobeat-lab evaluate-anchor-calibration INPUT.json CHOICE.json NEW_CALIBRATION.json
cocobeat-lab propose-anchors PACKAGE --calibration INPUT.json CALIBRATION.json CHOICE.json NEW_V2_PROPOSAL.json
cocobeat-lab adopt-anchor-proposal PACKAGE V2_PROPOSAL.json V2_SELECTION.json NEW_PACKAGE --calibration INPUT.json CALIBRATION.json CHOICE.json
```

Choice 的 schema 1 明确 `input_blake3`、`train_result_blake3`、`policy_index`；前者是 Input 原文件字节，后者是 typed Training 的 compact 序列化内容，区别于保存训练收据的原字节 hash。用户在训练后、评估前冻结选择，评估不重新拟合 bins；支持不足、分数超范围或判断 uncertain 时估计未知，缺少评估已知正负分母时 `not_applicable` 并拒绝提案

v2 提案将原 `confidence: null` 与独立 `calibrated_estimate` 分开保存，估计记录 probability / bits、原 scorebits、bin、方法和报告原字节 hash；原 analysis 与 native evidence 不修改。临时分析副本供既有选择器使用，confidence 降序 / time 升序和最小间隔保持，随后按从零帧起的固定半开窗口限制数量，末窗口截到 N，拒绝原因 `density_limited` 保留窗口与 capacity

V2 Selection 的 schema 2 明确来源 CID、实际保存提案原字节的 `proposal_blake3`、提案完整五字段 `calibration` context 和 `onset_indices`；采用前完整重算 Input / Training / Choice / Report / 提案，检查原来源和侧车新鲜性，只能选择已接受候选。空选择明确清空 Anchor，导出复用原事务并保留音频、analysis、SectionCue 和 native None；输出在全部声明 package / evidence 和实际传入的源包副本之外

旧 `--calibration` 分支只适用于 Input 中完全标注的 evaluation 包；未标注新 Source 使用下述显式 inference 入口，不回退到 evaluation。既有 v1 CLI / 报告保持，v1 工作台拒绝 v2；Input / Choice 各最多 1 MiB、校准报告最多 4 MiB，其他提案与选择沿用前述上限

2026-10-08 的 [软件记录](../testdata/synthetic/calibration-drift-observations-20261008.json)包含 Media86 / Lab96、Clippy / 格式 / 边界与当前 Lab 构建、33 条实际校准 CLI（24 成功 / 9 预期拒绝），46 条训练与 69 条新源评估候选、显式采用 1 个 Anchor、原 native reader 和 v1 完整字节回归；固定 index 构造标签验证机制，音乐准入、并发源副本写窗和真人试听仍未验收，首轮 lint 与 QA setup 失败保留

## 未标注新 Source 推断

```sh
cocobeat-lab infer-anchor-calibration NEW_SOURCE_PACKAGE EVIDENCE INPUT.json CALIBRATION.json CHOICE.json NEW_V2_PROPOSAL.json
cocobeat-lab workbench-candidates NEW_SOURCE_PACKAGE V2_PROPOSAL.json --inference EVIDENCE INPUT.json CALIBRATION.json CHOICE.json [--locale CODE]
cocobeat-lab adopt-inferred-anchors NEW_SOURCE_PACKAGE EVIDENCE V2_PROPOSAL.json V2_SELECTION.json NEW_PACKAGE INPUT.json CALIBRATION.json CHOICE.json
```

新 Source 不读取标签或重新拟合，原 Input / Training / Choice / heldout Report 全部重编核对后只复用冻结 bins 和预声明策略；新 canonical audio 必须不在 train / evaluation，原 native profile、选中声道和 score domain 一致，最多 1024 条原 None onset，支持不足或分数未知时估计仍为 None

v2 report 保留原五字段 calibration Context，以 `scope: "experimental_anchor_calibration_inference"` 和独立 `inference_source` 明确新 Source / analysis / native summary / profile / channel 的原字节绑定；原 frame、strength bits 和 confidence None 不变，`UNASSESSED` 与 `production_admission:false` 保持。V2 Selection 沿用 schema 2 的完整 source / proposal raw hash / Context 绑定，只能明确采用已接受候选，不自动补谱

只读工作台的 `--inference` 调用公共严格 `load_inferred_report` 重编新 Source 和原 Context，不静默切换到 evaluation；提出与采用均末次检查传入源副本及全部数据新鲜性，输出须在传入 Source、声明 package / evidence 和新证据之外，采用继续保留 audio / analysis / SectionCue 原字节

2026-10-09 的[推断软件记录](../testdata/synthetic/anchor-unlabelled-inference-observations-20261009.json)覆盖 Lab100 与五项检查、34 项独立 CLI（14 成功 / 20 准确拒绝，共 35 次尝试，首个缺 Choice 的 QA 前缀失配保留）、5 项公共严格 reader / CandidateView CPU 控制，以及新源推断 / 同 CID 副本 / 明确采用 / 原字节回归；机械新源 69 条原 None 候选、69 条独立估计、3 条政策选择和明确采用 1 个 Anchor 保持 UNASSESSED

公共 `workbench::run` 的单个 zh-CN、1280×800 窗口通过只读候选 / 详情、选择不自动播放、明确定位 197120 帧、Kira 游标 197632 / 暂停 199680 / 停止和关闭 exit0；正式 `--inference` 路由另有两项 before-App 拒绝，与原 34 项统计分开。两张图仅目检列表 / 详情可见，长 JSON 需要滚动，不承诺完整滚动、其他尺寸或实体输入验收

采用包使用原冻结 Game 和未变相关 runtime 输入，完整新 CID / 786432 帧、Ready 无音频、新确认后 Kira、暂停 / 恢复、两条合成 Hit 的 Replay / CSV、自然 exit0 和 worker 释放通过；这项软件出口不声称旧 Game 含新 Lab 代码。既有 MIR 质量 FAIL、历史 −9 UNKNOWN / 主动 −15 诊断、首次 lint 和两次 CPU QA 失败保留，真人音乐策略、实体输入、DAC / 扬声器与当前四平台新功能验收继续开放

## 校准候选只读工作台

```sh
cocobeat-lab workbench-candidates PACKAGE V2_PROPOSAL.json --calibration INPUT.json CALIBRATION.json CHOICE.json [--locale CODE]
```

打开前完整重算原 Input、训练与评估来源、Choice、校准报告和 v2 提案，再检查原字节新鲜性与实际传入的完整源包；可以传入相同内容的独立目录副本。缺少 context、篡改分数位、来源或政策均拒绝，旧 v1 入口仍只读取 v1

列表和波形保留全部原 onset 帧，详情分别显示原 strength / score bits / confidence、独立 probability / bits / bin / method / 报告来源、密度或间隔拒绝原因，以及完整 Source / policy / calibration context。原 chart Anchor 与提案选择分开显示，Unknown 继续留空，`UNASSESSED` 与 `production_admission:false` 保持

复用已有浏览、详情滚动和立体声试听；选择候选只移动浏览游标，定位按钮才请求试听跳转。该视图只读，增删、撤销、导出和帧编辑操作不改变原谱面；采用仍通过前述 CLI 明确选择

2026-10-08 的[消费者软件记录](../testdata/synthetic/calibrated-candidate-consumer-observations-20261008.json)包含 Lab99、Clippy / 格式 / 边界与构建、10 条正式 CLI 预期拒绝、5 条私有严格读取控制，以及调用公共 `workbench::run` 的单窗口验收。原包与同 CID 副本正向读取 69 条候选，55 条独立估计 / 14 条 Unknown / 3 条政策选择均保留原 None；同一窗口 1280×800 和 640×480 的六张图已按列表尾部、详情和滚动底部指定范围检查，实际 Kira 游标、暂停和停止状态通过软件断言

首轮新增测试的类型错误及 QA 分数位负例被前置 Input 哈希门拦截的结果保留，后者另补仅改变提案估计原分数位的正式 CLI 拒绝。物理输入、声学试听、真人音乐准入、更广泛的未标签推断 UI / 输入验收及真人审阅采用流程、并发改写窗口和当前四平台原生验收继续保留，窗口截图与软件状态不替代这些验收
