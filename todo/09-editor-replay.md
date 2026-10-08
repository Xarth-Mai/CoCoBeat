# 09 · 编辑器与 Replay

前置：已有编译内容和 02 的基础 Replay，此时创建 cocobeat-editor

- [x] 原生工作台显示双声道峰值波形、精确 SongTime、Anchor 列表与作者设置的 SectionCue
- [x] 显示 Anchor 候选、音乐结构证据与拒绝原因
- [x] 增删移动 Anchor，支持撤销重做，重新生成内容哈希，已由实际修包 CLI 消费
- [x] 图形界面显示 Replay 输入、水位、Anchor 判定与 Free Sync 配对，保留原始整数和事实关联
- [x] 三种工作台共用 canonical PCM 试听，独立显示源游标和请求起点，暂停 / 恢复 / 停止 / 选择跳转保留原编辑与 Replay 帧
- [x] 关联显式本机软件计时 sidecar，按精确 Replay 原字节和原 Hit fact_index 展示观察、消费等待、真实 mapping anchor 与独立音频历史；共享 / runtime / lab、实际 CLI 和原生详情软件验证通过，物理设备另验
- [x] 独立 Labels 编辑软件：隐藏作者 / 算法提示，共用最终立体声波形 / 试听，人工 point / interval 与三态判断、原生文本、Apply / Cancel 草稿和固定新侧车保存已接通；正式 57 项 Lab 测试、Clippy 与冻结构建通过，已补分版原生软件验证及最终法语小窗口指定范围目检，首轮视觉失败保留；OS IME / 设备 / 真人验收单独记录，见[独立标签](../docs/independent-labels.md#原生-labels-工作台)

- [ ] 物理输入、DAC / 扬声器与设备计时验收；Replay v1 / v2 不含设备 timestamp，软件消费等待不能从歌曲帧推算成物理延迟
- [x] 固定带版本/长度限制的 SongPackage / Replay 及 CLI 诊断契约，校验损坏与不支持版本
- [x] 编辑导出再载入无损，同一 core headless 重放结果一致；真实改谱必须使用匹配新身份的录制

退出条件：可以根据事实定位问题，Replay 不包含用户音频，开发遥测默认留在本机；调试 UI 不自动变成正式游戏 UI

## 当前实施批次

2026-10-03 已实现纯 Anchor 编辑内核、原字节保真包导出和 `edit-anchors` CLI：精确帧增删移动、有限撤销重做、绑定源包完整身份，全部操作成功后导出新包；音频与 analysis 原字节保留，未编辑 cue 保持，实际修改才重算身份，无变化导出保留原身份

该批 47 项相关测试、Clippy/构建、35 项真实包/CLI 用例及 1 张实际改谱 GPU 图通过，来源身份、原字节保真、错误保留与旧 Replay 边界见 [验证策略](../docs/testing.md#精确-anchor-编辑与原字节保真导出)

后续 `inspect-replay` 已接通真实歌曲包的有界 JSONL 诊断，原始 Hit / 水位、已确认事件、精确整数关联与最终 Resonance 来自同一 core；20 项相关测试、4 项断言补强后的定向补验及 20 项实际 CPU 用例通过。空或单方历史不补 Miss，原包 / 无变化 / 撤销导出报告相同，真实改谱拒绝旧身份，具体接口见 [Replay 诊断](../docs/replay-diagnostics.md)

原生 `workbench PACKAGE NEW_PACKAGE [--locale CODE]` 已消费相同编辑与导出路径，支持精确帧输入、拖动、撤销重做、虚拟列表、单一菜单主控及失败保留重试；手柄可浏览，编辑与导出由键鼠完成，现有 13 个语言变体和 Noto 字体共用，配置只读

设备计时关联、试听校准及完整阶段退出继续保留；早期静音工具的窗口检查不能替代音频、设备或真人验收，软件证据见 [工作台验证](../docs/testing.md#原生波形与-anchor-工作台)

2026-10-05 完成原生波形工作台软件里程碑并提交后暂停：本轮 35 项测试、10 条 CLI 命令及 8 张原生 GPU 图通过，旧临时证据缺失的边界与当前补验见 [工作台验证](../docs/testing.md#原生波形与-anchor-工作台)；本阶段未勾选任务和完整退出继续保留

2026-10-07 完成 `workbench-replay PACKAGE REPLAY [--locale CODE]` 只读图形诊断，共用完整校验、唯一 core 重放、波形和虚拟列表；可逐条定位 Hit / 水位 / 判定 / 配对，负预滚和 EOF 后水位保持原值，小窗口详情可滚动到底，编辑与导出动作关闭

本批相关包共 169 项测试、Clippy / 构建通过，1280×800 和 640×480 的源码副本 helper 各核对 76 条记录并生成 5 张原生 GPU PNG，合计 10 张图目检通过；正式 CLI 的两份报告与 stdout 保持旧版原字节，命令、冻结源码与合成输入边界见 [Replay 诊断验证](../docs/replay-diagnostics.md#软件验证)和[本批观察清单](../testdata/synthetic/session-diagnostics-observations-20261007.json)

该里程碑不包含试听、物理计时或真实设备验收，Replay v1 也没有视觉版本，当前诊断不承诺历史画面复现；v2 几何版本记录与 `inspect-replay-stage` 保留此边界，该批尚未实施动画观看，后续交付见下文；软件交付完成后等待用户真实验收，长期 V1 的设备计时关联和试听校准范围保留

2026-10-07 完成 `workbench-candidates PACKAGE REPORT [--locale CODE]` 只读候选审阅，共用 CLI 的报告版本、完整源包身份、规则与策略校验及候选重编；逐条显示精确帧、原始评分、零起始索引及阻挡关系，beat／section／energy 上下文来自原 analysis，提案与原 chart 分开显示，采用仍由明确选择 CLI 完成

28 项实际 lab 测试通过，13 份各 206 key 文案及占位符一致；1280×800 和 640×480 各核对 40 条候选，640×480 另核对零候选，11 张原生 GPU PNG 经 agent 与主线程目检通过。该批是冻结生产模块副本、受控输入和确切旧 debug extern 的 `rustc` 检查，依赖含当时未提交但本入口未调用的 Stage 新 API，不代表整个当前提交的 Cargo 构建；先前测试、QA 类型、socket 与首屏视觉失败及修复保留，命令和哈希见[持久观察清单](../testdata/synthetic/candidates-workbench-observations-20261007.json)及[复现说明](../tools/candidates-workbench-check/README.md)

主线程在当前工作区另完成正式 Cargo 的 28 项 lab 测试、Clippy、全仓格式与依赖边界检查；这份补验包含当时未提交的 Stage 新 API，与固定 GUI 源码副本分别取证

构造 onset／beat／section 是未校准机制控制，不能作为 MIR 质量准入；本批不包含试听、设备计时关联、物理手柄或母语使用者验收，完整阶段退出继续保留

2026-10-07 · 歌曲试听软件交付：首次播放才创建 Kira 输出，17 项最终工作台窄测覆盖命令合并、原始帧保留、菜单主控、held、失焦和断开；实际 Kira MockBackend 起点与暂停 / 恢复检查通过。两尺寸四张原生 GPU 图目检通过，音频游标是构造显示控制，输出设备和物理输入为 NOT RUN，见[操作契约](../docs/editor.md#歌曲试听)与[观察清单](../testdata/synthetic/workbench-audition-observations-20261007.json)

2026-10-07 · 试听真实 callback 补验：冻结生产模块和旧 extern 的 helper 实际走 Kira CPAL，两个尺寸正常播放 / 暂停 / 暂停跳转 / 恢复 / 失焦 / Stop 与一个实际输出初始化失败共三 case 通过，七张实际图 agent / 主线程目检通过；原选中事实和源包保持，见[独立记录](../testdata/synthetic/workbench-audition-callback-observations-20261007.json)。callback 状态不证明声学输出或物理计时

2026-10-07 · 原生只读视觉 Replay 已交付：`--watch-replay` 使用明确 Stage 1 / 2、原事实顺序与实际 Kira acknowledged cursor，完整开场后明确确认才播放；暂停、恢复、原 epoch 重启及返回菜单保持原包 / Replay，完整与 partial 不补造历史。133 项 runtime 测试、最终标题窄测、Clippy / 格式通过，四个实际窗口 case 与最终三张静态标题图分别绑定冻结二进制；证据和历史失败见[观看记录](../testdata/synthetic/visual-replay-observations-20261007.json)，历史 shader、设备计时及真人验收另计

2026-10-08 · 本机 timing sidecar 已接通正式 `inspect-replay … --timing SIDECAR` 和 `workbench-replay … --timing SIDECAR [--locale CODE]` 消费路径；显式报告 v3，未传时维持 v1 / v2 原内容，旧 CSV 不自动猜关联。core 9 项、replay 11 项、runtime 174 项与 lab 32 项检查通过，32 条实际 CLI、两组原生游戏与原生详情补验通过；首次 QA 焦点失败及指定范围三图目检见[观察记录](../testdata/synthetic/timing-sidecar-observations-20261008.json)，接口与原文件保护见[显式计时关联](../docs/replay-diagnostics.md#显式本机计时关联)
