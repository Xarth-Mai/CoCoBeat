# 独立人工标签

`label-source PACKAGE` 完整验证已有 SongPackage 并只读输出来源；`import-labels PACKAGE INPUT NEW_DOCUMENT` 验证人工 JSON 后另存新标签；`compare-labels PACKAGE LEFT RIGHT NEW_REPORT` 比较两位不同本地别名的明确交集并保存新报告。这些命令处理独立侧车文件，保持音频、analysis、chart 和 manifest 原字节，不创建自动 onset / Anchor / confidence

```sh
cocobeat-lab label-source /path/to/package
cocobeat-lab import-labels /path/to/package /path/to/reviewer-a-draft.json /path/to/new-reviewer-a.json
cocobeat-lab import-labels /path/to/package /path/to/reviewer-b-draft.json /path/to/new-reviewer-b.json
cocobeat-lab compare-labels /path/to/package /path/to/new-reviewer-a.json /path/to/new-reviewer-b.json /path/to/new-comparison.json
```

`import-labels` 成功 stdout 新增 `labels_blake3`，它是本次成功保存的新文档完整原字节 BLAKE3，包含格式与末尾换行，不是输入草稿的 hash；供下文明确采用时绑定同一个文件

输出父目录必须存在，目标文件必须尚不存在且在源包外；父目录 alias 指向包内也拒绝。保存前重新完整验证包并要求初始与最新 Source 全等，来源漂移拒绝输出，已有标签 / 报告不会被覆盖

输入为 UTF-8 JSON，schema_version=1，source 逐字段使用 `label-source` 的完整对象，reviewer 为 1–64 UTF-8 字节且无首尾空白的本地别名；文档最多1MiB和1024条label，未知或重复字段拒绝。source 绑定完整 package content ID、audio BLAKE3、N 和 canonical_decoded basis，本批不能把原来源 PCM 的记录混入 canonical 基准

每条记录包含人工共享的 item_id:u64、location、anchor_decision、reason 和 playback。location 为 point 的 frame，或 interval 的 start_frame / end_frame；point 满足 `0 <= frame < N`，interval 为 `0 <= start < end <= N` 的半开区间，EOF 不是攻击点。anchor_decision 只能为 should_anchor / should_not_anchor / uncertain，reason 必须非空且最多2048 UTF-8字节，playback 当前仅接受实际可用的 stereo

位置和判断由审阅者明确输入，不把来源trigger、已有Anchor、MIR、鼠标 / 手柄时间戳或音频callback当作听觉 onset。单条记录表达该处的人工 Anchor 判断，不产生音乐分析事实；工具验证数据与身份，不能证明填写者听过音乐或已取得听觉共识

双人比较先要求相同完整Source与不同reviewer别名，仅按明确相同item_id比较；报告保留双方原位置、判断、理由、播放方式、每项相同与否及左右unmatched，最多4MiB。临近时间但不同ID不自动配对，分歧不融合、投票、校准confidence或自动导出谱面

普通 `workbench` Edit 显示创作 Anchor，因此试听后在外部 JSON 记录是有谱面上下文的路径；新增 `workbench-labels` 专门隐藏创作 / 算法提示并在原生界面编辑独立标签，功能与本批软件边界见下文。两种路径都不能据软件实现证明审阅者实际听过音乐或取得共识

2026-10-08 CLI 批次当时的验证记录：Lab 40 项测试、近上限报告 1 项重复窄测、最终全目标 Clippy 与构建通过；固定 64 秒合法包的 33 项真实 CLI 控制为 PASS_SOFTWARE_CLI_CONTROLS，来源四对象、原标签和已有输出字节保持。构造记录只验证标签机制；该 CLI 批次当时尚未实现 GUI 标签编辑 / 隐藏提示盲标，人类独立标签、听感、可玩性、置信度校准、外部音乐与其他平台 CLI 运行仍为 NOT_RUN；CLI 加载至写出期间完整来源替换竞态未实跑，fresh-source helper 的另一合法包拒绝有窄测

软件证据位于 `target/independent-label-delivery-20261008/`：`lab-tests.log`、`near-limit-recheck.log`、`clippy.log`、`clippy-recheck.log` 分别保留完整结果。首次 Clippy 仅因两处测试断言 `len() + 1 <= MAX_BYTES` 触发 `int_plus_one` 而失败，改为等价的 `len() < MAX_BYTES` 后复查通过，生产逻辑未改变；40 项是完整 Lab 测试数量，近上限 1 项为复跑

本批 CLI 使用 `build-02/bin/cocobeat-lab` 与对应 `build-02/cocobeat-lab-build-result.json`，449 项构建输入冻结一致，二进制 SHA-256 为 `45975720d9310cb536279bd3943edb6de8e50740b18c6ed803ddd2da8f7ee51b`。早期 `build/` 及最终 `build-02/` 的测试源码快照不同，即使非测试二进制 SHA 相同也不把两套输入当作同一版本

`cli-controls-01/summary.json` 保留每次命令、退出码与 stdout / stderr SHA，共 33 次命令、66 份原始日志；覆盖正常来源读取 / 导入 / 明确 ID 比较、非法身份 / 字段 / 位置 / 数量 / 大小、同审阅者、源包内及 alias 输出拒绝、FIFO / 目录 / symlink 输入静态拒绝和目标防覆盖。无超时，原包与副本的四对象、标签输入和已有结果字节保持；该覆盖不构成并发路径替换防护证明

近上限两份合法输入各为 1,008,583 字节，报告为 2,102,012 字节，超过 2 MiB 且小于 4 MiB；448 条明确交集的左右全部原记录逐条比较一致，报告 SHA-256 为 `0bce3f77c2d38369c94dfc38d9646aa08c7f97aceff6ec4896292c4028d9d136`

正式[验收记录](../testdata/synthetic/independent-label-observations-20261008.json)与[原始索引](../testdata/synthetic/independent-label-raw-index-20261008.json)绑定本批软件输入及留存证据

## 原生 Labels 工作台

`workbench-labels PACKAGE NEW_LABELS_JSON [--locale CODE]` 完整验证源包并打开独立人工标签模式，使用同一 final canonical 双声道 PCM 的波形与 Kira 试听；界面不展示作者 Anchor、SectionCue / 结构文字、模型候选及评分。工作台仍只读语言配置，13 个完整语言代码与 Noto Sans 共用，不改游戏设置

```sh
cocobeat-lab workbench-labels /path/to/package /path/to/new-reviewer-a.json --locale zh-CN
cocobeat-lab compare-labels /path/to/package /path/to/reviewer-a.json /path/to/reviewer-b.json /path/to/new-comparison.json
```

每次打开建立空标签文档，填写本地 reviewer 别名，选择光标位置新增记录或选择已有记录修改 / 删除；新增时分配最小未用 item_id，编辑保留该 ID。GUI 不载入已有标签文件，双人比较仍只认双方明确相同 item_id，比较前须确认 ID 指向同一审阅项，不能把各自新增次序自动当作同一音乐事件

位置字段接受明确的非负 ASCII 整数帧，point 满足 `0 <= frame < N`，interval 为 `[start_frame, end_frame)` 且满足 `0 <= start_frame < end_frame <= N`；选择 kind / decision 字段后左右键切换点 / 区间及应 / 不应 / 不确定 Anchor，Tab / Shift+Tab 切换焦点。reviewer 和 reason 使用 Bevy 原生 EditableText；保存契约继续按 UTF-8 字节限制别名 64、理由 2048、文档 1 MiB 和最多 1024 项，不将字符数代替字节数

新增 / 编辑使用独立草稿，Apply 验证后才更新标签，Cancel 丢弃当前记录草稿；未应用的真实文本同样进入 dirty 判断。尚有记录草稿时必须先 Apply 或 Cancel 才能保存；IME composition / pending edit 尚未稳定时，Apply / 保存等待实际原生文本同步，输入门控保留单一菜单主控、失焦 / held 与文本编辑的命令消费顺序，手柄用于浏览 / 试听，记录编辑和保存由键鼠主控执行

「保存并退出」只写启动命令指定的新 JSON 文件，父目录须存在，目标在源包外且不能覆盖已有路径；后台 worker 重新完整核验源包并要求初始 Source 全等，再用 create_new 保存，主线程 join 后才处理成功退出或保留错误 / dirty 文档。保存期间关闭不会分离写入线程，未保存修改退出需明确确认；工具不把输入时刻或音频 callback 转成听觉 onset，也不将记录自动采用为 Anchor

本批正式 Lab 57 项测试、最终全目标 Clippy、格式检查及 465 项冻结输入构建通过，独立静态正确性 / Pony 审阅无开放 findings；旧 CLI 的 40 项测试与 33 项命令实绩独立保留。最终 `build-04` Lab 二进制 SHA-256 为 `dc02f41377c8ea008bb7dfb4b41fc6c69fce69db42802eece9c5c4cc6fd9ba06`，构建 receipt SHA-256 为 `d6fca55014c184e6d342774bb2ceb016cdcb828999f149223e88ad258d380ad5`，正式[观察记录](../testdata/synthetic/independent-label-ui-observations-20261008.json)与[原始索引](../testdata/synthetic/independent-label-ui-raw-index-20261008.json)分别保留各版输入、实际命令和范围

原生软件补验分版保存：首轮 4 项软件 case 通过、14 张 PNG，1280×800 指定范围目检通过，640×480 视觉可用性失败；修复压缩 Labels 工具栏 / 波形占用并按真实文本行高布局后，二轮 5 项软件 case 通过、17 张 PNG，其中 11 张按指定范围目检，不能据此宣称全部截图目检。随后原生词边界换行和法语编辑路径补验 1 项 / 4 张图，再清理未用文案后以最终二进制补验法语 640×480 的 1 项 / 4 张图，两个法语批次各四图均由主线程实际查看；总计 11 项原生软件 case、39 张 PNG，旧失败保持

最终法语理由字段高 107 px，完整位于 123 px 详情面板内，标题、两行文本、光标与焦点边框可见，工具栏完整词可读；刻意滚至最大值时裁去上方理由部分是该滚动位置的结果，不能从这一张底部图推断整字段可见。实际编辑焦点图与底部 Apply / Cancel 图分别验收，最终版本仅补了法语小窗口，先前中文及大窗口结论绑定各自旧版，不能平移成最终版本全部语言 / 尺寸通过

字形历史按原版保留：当时 13 个 locale 各新增 39 条文案，首轮 Noto cmap 为 5 PASS / 8 FAIL，缺少的 `≤` 改成 ASCII `<=` 后第二轮 13 PASS。最终删除每个 locale 的 8 条未用文案，保留 31 条有效 Labels 文案和原 252 条文案值，实际最终 13 × 31 字形检查通过；字形覆盖不替代母语、任意 Unicode 或真实布局验收

原生窗口实际运行使用软件 KeyboardInput / Ime 消息经过正式输入、EditableText 和保存 worker，导出的构造标签经正式 CLI 回读，已有目标保护和自有进程组 / helper 退出有记录；这不是实际 OS 输入法或真人操作。OS IME、物理双手柄 / 混合输入、母语审阅、声学试听 / DAC 计时、原生窗口忙 worker 关闭、真人隐藏提示盲标与 confidence 校准仍为 NOT_RUN，该 UI 批次当时未接标签采用；后续 CLI 见下文，软件支持不作为实际音乐质量或真人共识

## 明确采用为 Anchor

`adopt-labeled-anchors PACKAGE LABELS SELECTION NEW_PACKAGE` 将明确选中的人工肯定点替换为新包的完整 Anchor 列表；它消费独立标签，不经过 onset 候选阈值或推算 confidence。标签工作台仍只保存侧车，采用使用此 CLI，没有新增 UI 采用按钮

先保存审阅记录；若记录来自工作台或外部 JSON，用 `import-labels` 另存为准备用于采用的文件，并保留成功 stdout 的 `source.content_id` 与 `labels_blake3`

```sh
cocobeat-lab import-labels PACKAGE REVIEWED_LABELS.json ADOPTION_LABELS.json
```

选择 JSON 使用上述完整 Source CID、保存后文件的 raw BLAKE3 以及明确 item_id；以下 `11` 是示意，须替换为这份标签中实际要采用的 ID

```json
{
  "schema_version": 1,
  "source_content_id": "package-blake3:<完整64个小写十六进制字符的包哈希>",
  "labels_blake3": "<ADOPTION_LABELS.json原字节的64个小写十六进制字符BLAKE3>",
  "item_ids": [11]
}
```

```sh
cocobeat-lab adopt-labeled-anchors PACKAGE ADOPTION_LABELS.json SELECTION.json NEW_PACKAGE
cocobeat-lab verify-package NEW_PACKAGE
cocobeat-lab inspect-stage NEW_PACKAGE 0
```

LABELS 与 SELECTION 均为最多 1 MiB 的普通文件，读取前及打开后检查文件类型；选择 schema_version 必须为 1，最多 1024 个 u64 item_ids，未知 / 重复字段、版本或身份不符均拒绝。标签从同一次有界读取的原字节解析并计算 hash，仅改变 JSON 空白也必须更新选择所绑定的 hash，不对重序列化结果猜测原文件身份

只允许选择 `should_anchor` 且 `location.kind = point` 的记录，整数 frame 保持原值，item_id 保留为 Anchor ID，最终按 frame 升序排列；未选择的记录不自动加入，缺失 / 重复 ID、所选点重复 frame、`should_not_anchor`、`uncertain` 和 interval 全部拒绝，不取区间中点、合并近邻或自动解决双人分歧

**item_ids 表示完整的新 Anchor 列表，空数组明确清空全部 Anchor**；它不与旧谱面合并，也不改变原标签。选择重复 frame 的拒绝是本采用入口的规则，既有通用 Anchor 编辑与包 schema 的合法范围保持原契约

源包先完整验证，标签 Source 必须同时匹配完整 content ID、audio BLAKE3、N 与 canonical_decoded basis，选择再绑定 Source CID 和标签原字节。导出前 fresh Source 全等检查后复用 `media::export_anchors` 的 expected CID 与事务导出，目标父目录须存在、位于源包外，已有路径和指向包内的父目录 alias 拒绝；这些检查不构成 CLI 读取至发布期间并发来源替换已验收，完整时序替换仍 NOT_RUN

新包保留 `song.audio.ogg` 与 `analysis.bin` 原字节（包括 sections 和未知能力状态），保留 chart 的 SectionCue / rules；实际 Anchor 改变才重写 chart 与 manifest、生成新 CID，完全无变化则保留四对象原字节与原 CID。既有 Stage v2 从未改动的 analysis.sections 生成相同几何，计划仍绑定新包 CID；旧 Replay 不自动迁移到改谱后的身份，新包由原 `--package` / 曲库加载入口消费

成功 stdout 给出源 / 新 CID、labels_blake3、reviewer、按成谱顺序的 item_ids、Anchor / SectionCue 数量、changed、`scope: "explicit_manual_point_adoption"` 和 `production_admission: "not_assessed"`；理由与审阅判断保留在原标签文件，可按 hash 回查，不把侧车或新字段塞入四对象包

2026-10-08 本批正式 Lab 65 项测试（新增 8 项采用窄测及原有 57 项）、最终 Clippy / 格式和 466 项输入冻结构建通过；首次格式检查因 `write_new` 的 Hash 返回签名换行失败，格式修复不改变 token / 行为，原失败与复查结果分别保留。冻结二进制与准确源码身份见[采用观察记录](../testdata/synthetic/label-adoption-observations-20261008.json)，逐命令输出及文件身份见[原始索引](../testdata/synthetic/label-adoption-raw-index-20261008.json)

固定 64 秒包的 46 条真实 CLI 控制通过：27 条成功、19 条正确拒绝，92 份 stdout / stderr 保留且无超时；来源、标签原字节 hash、肯定点选择、空选择与 no-op、错误选择 / 路径 / FIFO、输出防覆盖均符合预期，原包、副本、原标签和已有输出字节保持。五个代表位置的 Stage v2 采样除 CID 外逐字段相同；新身份的构造 Replay 保留四条事实，双方对 item_id 11 / frame 3000 产生 precise 判定，旧 Replay 在新包上拒绝，空包没有 Anchor 判定

构造标签和 Replay 只验证软件机制；完整 CLI 中途来源替换、该入口的原生游戏 / GUI 与其他平台运行、人工标签、听感、可玩性及 confidence 校准仍 NOT_RUN，原 MIR 算法质量 FAIL 和 confidence=None 保持
