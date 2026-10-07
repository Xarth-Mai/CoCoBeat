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

可以复用现有工作台波形、整数光标和canonical歌曲试听后在普通JSON编辑器记下记录。当前workbench Edit显示创作Anchor，因此这是有谱面上下文的人工记录路径，不能算隐藏提示盲标；未来Labels模式才隐藏创作 / 算法提示，GUI内标签编辑与IME仍未交付

2026-10-08 软件验证通过：Lab 40 项测试、近上限报告 1 项重复窄测、最终全目标 Clippy 与构建通过；固定 64 秒合法包的 33 项真实 CLI 控制为 PASS_SOFTWARE_CLI_CONTROLS，来源四对象、原标签和已有输出字节保持。构造记录只验证标签机制，人类独立标签、听感、可玩性、置信度校准、外部音乐、GUI 标签编辑 / 隐藏提示盲标和其他平台 CLI 运行仍为 NOT_RUN；CLI 加载至写出期间完整来源替换竞态未实跑，fresh-source helper 的另一合法包拒绝有窄测

软件证据位于 `target/independent-label-delivery-20261008/`：`lab-tests.log`、`near-limit-recheck.log`、`clippy.log`、`clippy-recheck.log` 分别保留完整结果。首次 Clippy 仅因两处测试断言 `len() + 1 <= MAX_BYTES` 触发 `int_plus_one` 而失败，改为等价的 `len() < MAX_BYTES` 后复查通过，生产逻辑未改变；40 项是完整 Lab 测试数量，近上限 1 项为复跑

本批 CLI 使用 `build-02/bin/cocobeat-lab` 与对应 `build-02/cocobeat-lab-build-result.json`，449 项构建输入冻结一致，二进制 SHA-256 为 `45975720d9310cb536279bd3943edb6de8e50740b18c6ed803ddd2da8f7ee51b`。早期 `build/` 及最终 `build-02/` 的测试源码快照不同，即使非测试二进制 SHA 相同也不把两套输入当作同一版本

`cli-controls-01/summary.json` 保留每次命令、退出码与 stdout / stderr SHA，共 33 次命令、66 份原始日志；覆盖正常来源读取 / 导入 / 明确 ID 比较、非法身份 / 字段 / 位置 / 数量 / 大小、同审阅者、源包内及 alias 输出拒绝、FIFO / 目录 / symlink 输入静态拒绝和目标防覆盖。无超时，原包与副本的四对象、标签输入和已有结果字节保持；该覆盖不构成并发路径替换防护证明

近上限两份合法输入各为 1,008,583 字节，报告为 2,102,012 字节，超过 2 MiB 且小于 4 MiB；448 条明确交集的左右全部原记录逐条比较一致，报告 SHA-256 为 `0bce3f77c2d38369c94dfc38d9646aa08c7f97aceff6ec4896292c4028d9d136`

正式[验收记录](../testdata/synthetic/independent-label-observations-20261008.json)与[原始索引](../testdata/synthetic/independent-label-raw-index-20261008.json)绑定本批软件输入及留存证据
