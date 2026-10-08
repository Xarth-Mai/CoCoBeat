# 独立人工标签

`label-source PACKAGE` 完整验证已有 SongPackage 并只读输出来源；`import-labels PACKAGE INPUT NEW_DOCUMENT` 验证人工 JSON 后另存新标签；`compare-labels PACKAGE LEFT RIGHT NEW_REPORT` 比较两位不同本地别名的明确交集并保存新报告。这些命令处理独立侧车文件，保持音频、analysis、chart 和 manifest 原字节，不创建自动 onset / Anchor / confidence

```sh
cocobeat-lab label-source /path/to/package
cocobeat-lab import-labels /path/to/package /path/to/reviewer-a-draft.json /path/to/new-reviewer-a.json
cocobeat-lab import-labels /path/to/package /path/to/reviewer-b-draft.json /path/to/new-reviewer-b.json
cocobeat-lab compare-labels /path/to/package /path/to/new-reviewer-a.json /path/to/new-reviewer-b.json /path/to/new-comparison.json
```

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

原生窗口实际运行使用软件 KeyboardInput / Ime 消息经过正式输入、EditableText 和保存 worker，导出的构造标签经正式 CLI 回读，已有目标保护和自有进程组 / helper 退出有记录；这不是实际 OS 输入法或真人操作。OS IME、物理双手柄 / 混合输入、母语审阅、声学试听 / DAC 计时、原生窗口忙 worker 关闭、真人隐藏提示盲标与 confidence 校准仍为 NOT_RUN，标签采用尚未接线，软件支持不作为实际音乐质量或真人共识
