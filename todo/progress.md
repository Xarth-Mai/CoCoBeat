# 工作进度

更新：2026-10-08，用户恢复完整项目推进并允许 subagent 并行，本批已完成 QUIC 四对象接收、网络时钟、原生游戏接线、正常结束后同进程新局及只读 Replay 图形诊断，同 epoch 原音源软件恢复已取得新证据，继续漂移 / 故障矩阵、生产源入口与内容路线。第一批规则、时钟、Replay、原创内容和 runtime 已通过完整软件检查，GPU 离屏截图已检查；品牌模块已交付、主线程已接线，品牌细节仍持续迭代，共享文件由主线程统一集成，实际证据统一记录在 [验证策略](../docs/testing.md)

本表记录分工与证据，阶段范围仍以 [00–12 路线图](README.md) 为准；研究报告提供设计来源，不代表库兼容性、硬件精度或真人体验已验证

当前执行顺序见 [重新制定的目标](current-goal.md)：26 的设置闭环与 29 的基础窗口接线已完成软件验证，27 的 13 语言接线已提交，28 的画质与帧率实现通过软件与 GPU 检查，29 的设置滚动与工作区适配已完成软件检查，极小 Ready 菜单已修复，继续取得平台显示验收；品牌交付和构建证据并行推进；用户已批准后续软件开发与设备/真人验收并行，完整 V1 的实际验收门槛保持不变

`IN PROGRESS` 表示正在实施，`NOT RUN` 表示尚未执行或验收，`PASS` / `FAIL` 需附实际命令或报告，`BLOCKED` 需写明缺少的条件；代码完成、模拟通过和真实设备验收分别记录

| 工作包 | 工作与阶段 | 负责人 | 状态 | 前置 | 交付证据 |
|---|---|---|---|---|---|
| 01 | 工程底座的远端 CI 与四目标首次构建 · 00 | root | PASS | 既有底座 | 0340ea9 的轻量 CI PASS；Linux 两架构在 4c63dcc、Windows 两架构在 34eaf5b 原生构建及下载包静态核验 PASS，Windows 首轮换行失败已修；新 tag 发布流程本地检查通过，实际发布仍单独取证，见构建记录 |
| 02 | schema 输入事实、Anchor 判定、Free Sync 与 Resonance · 02 | core_engine | PASS | 既有 SongTime | schema/core 独立测试通过，包含 720 种交付排列；体验参数未验收 |
| 03 | 软件时钟映射与失效状态 · 01 | clock_bridge | PASS | 既有 SongTime | ClockBridge 7 项软件测试；不代表真实设备精度 |
| 04 | 输入持久化与同一 core 重放 · 02/09 | replay_engine | PASS | 02 的事实与规则契约 | Replay 5 项测试，含 60/144 Hz 与 500 ms 卡顿批次、损坏拒绝和保存失败 |
| 05 | 软件计时模拟与统计 · 01 | timing_lab | PASS | 03 的映射契约 | 实际运行 16 情景、198,816 个采样，0 个超出声明的不确定性；仅软件模拟 |
| 06 | 原创 64 秒内容与独立帧标注 · 02 | timing_lab | PASS | 既有 SongTime | 长度、PCM 字节、静默、7 个手写 Anchor 和独立标注通过检查；听感未验收 |
| 07 | Bevy/Kira 输入与音频 API 核实 · 01/02 | runtime_research | PASS | 当前依赖版本 | 音频 API、异步播放状态、设备错误与游标边界已核实；不作为设备验收 |
| 08 | runtime 接入、当前批次集成与验证 · 01/02 | root | PASS | 02–07 的接口逐项就绪 | 本地切片基线的完整 workspace 40 项测试、边界/格式/Clippy 通过，game/lab 实际构建通过；真实音频交互属于 11/12 |
| 09 | 键盘/手柄流程与最小双人表现 · 02 | input_capture + neon_view | PASS | 08 | 输入边沿/归属软件测试、双角色与 HUD 的 GPU 离屏截图通过；真实设备仍为 12 |
| 10 | 本地切片软件确定性验收 · 02 | root | PASS | 04/06/08/09 | 不同批次 Replay 与 Session 捕获测试通过；实际 CLI 的合成 18 facts 得到 22 events，错配/截断均拒绝；真实运行属于 11/12 |
| 11 | 真实输入/音频计时与校准 · 01 | root + 设备测试 | NOT RUN | 03/05/07/08 | loopback/硬件报告、设备与构建信息、偏移/漂移/不确定性，依据实测确定窗口 |
| 12 | Windows/Linux 平台与手柄验收 · 02 | root + 设备测试 | NOT RUN | 09/10/11 | 双键盘、双手柄、混合输入、菜单、重绑定、USB/蓝牙及重连记录 |
| 13 | 真人双人体验与规则反馈 · 03 | root + 测试参与者 | NOT RUN | 10/11/12 | 对照顺序、实际行为、访谈、沉默/模仿/连点与打乱输入对照；失败返回 02 |
| 14 | 雨夜霓虹表现完善 · 04 | neon_next + root | IN PROGRESS | 软件按用户批准并行，完整退出需 13 | 已交付双耳/单冠角色、街道纵深、五种反馈及 Precise/Good 强度；9 张静态样例和 240 张真实 core 驱动连续 GPU 帧取证通过，Resonance 仅改变独立招牌；真实性能及真人体验继续保留 |
| 15 | 唯一标准音频导入与编码回读 · 05 | media agents + root | IN PROGRESS | 软件按用户批准并行，完整退出需 13 | 源解码、High 重采样、严格最终读回和音频 staging 已接入 lab；原生 vorbis_rs 编码软件入口已交付，39 项 media 测试、14 项完整双回读、38 项质量控制与修补后 11 项 Sanitizer，四目标准入继续推进；全频带固定 19 例及新增 4 例数值域组合均保留双路读回、正式 guard 拒绝与局部质量退步，编码准入、曲库及听感继续推进 |
| 16 | SongPackage 身份与原子 Ready · 05 | root + media agents | IN PROGRESS | 15 | 初始四对象包、有界 Postcard、版本头、BLAKE3 和原子目录发布已实现；能量从严格读回的同一 staging 副本测量，运行时已消费最终 PCM、实际长度、手工 Anchor、SectionCue 和完整包 Replay 身份；段落批次 103 项 runtime 测试、13 项 CLI 与 10 张 GPU 图通过，先前 152 项 workspace 检查独立保留；Ready 手工包曲库另已补六项原生软件与 42 图指定范围检查；Ready 配对源入口已通过 164 项 runtime 测试、七项实际导入与同图六项旧包回归，五张源导入图指定范围目检，完整 MIR/Anchor/Stage 输出仍待后续 |
| 17 | MIR 基准与人工标注 · 06 | MIR agents + root，标注参与者待落实 | IN PROGRESS | 原始 PCM 基准先行，最终回读对照需 15 | 原创来源清单与独立标签侧车已交付，人工字段仍 pending；显式原生 canonical CPU beat 候选完成 media 46 / xtask 5、12 条成功命令、四组同实际 spect 对照及 13 项事务 / 资源控制，confidence=None，不自动 Anchor；原门控质量与 wholeSpect / 音乐 FAIL 保留，完整 MIR 继续推进；Labels 隐藏提示编辑软件已实现，分版原生窗口软件补验与最终法语小窗口指定目检通过，OS IME / 设备、真人盲标与校准继续推进；原生 beat / raw downbeat 只读消费者及分版软件验证已交付，图例视觉失败 / 定向修复保留，见[工作台](../docs/native-beat-candidate.md#原生候选工作台)，音乐准入与设备 / 真人边界保持；[MusicTruth](../docs/music-truth.md) 四 CLI 人工事件软件入口另补 20 条真实控制，coverage 交集精确帧分歧与音频身份保留，真人音乐参考 / 置信校准仍待完成；[40 项原生矩阵](../docs/native-beat-candidate.md#40-项导入与证据矩阵)完成导入 / reader 软件覆盖，原合成日程机械对照保留误报 / 漏检，旧 SDK 会话副产物证据保留，音乐质量 FAIL 与真人 / 校准仍未关闭；[Linux 实验导入取消](../docs/native-beat-candidate.md#ctrlc-取消原生候选导入)七项控制通过，真实 SDK 原 Err、同 PID fresh API 重试、600 秒前处理检查点零发布与初始化 guard 均按实测范围记录，Ready / GUI / Windows 和音乐质量另验；[原生候选覆盖对照](../docs/music-truth.md#对照原生候选)完成三流显式容差机械报告，7 项窄测 / 79 项 Lab 与 16 项实际 CLI 通过，构造参考不作为真人真值，音乐质量仍未准入；[原生 tempo 观察](../docs/native-tempo-candidate.md)完成固定绑定 / canonical 只读原值报告与单包两声道等价软件验证，UNSCORED / unit / meter / confidence 未知，首轮 QA FAIL 保留，固定 `8cdda9d` 四目标优化包及十二项 Lab smoke 已通过，可靠 TempoRegion 和音乐质量另验；[原生谱形诊断](../docs/native-structure-features.md)已交付固定窗 / 真实 EOF / 至多4个相似邻居的只读 JSON，生产 media72 / Lab82及 Clippy / 格式 / 边界通过；12条候选快照 CLI 与600秒成本另绑原快照，音乐语义未准入，新入口四目标原生 NOT_RUN |
| 18 | 精度优先的 AnchorCompiler · 07 | editor_core + stage_curve_plan + section_qa + root | IN PROGRESS | 现有分析结构可先行；生产策略依赖 17 | 纯 AnchorProposal、逐项证据、完整重编及明确采用 CLI 已交付；41 项相关测试和 32 项真实包 CPU 检查通过，原音频/分析/cue 保真，core 与 Replay 一致；音乐置信度校准、标注与试听继续保留 |
| 19 | 确定性 StageCompiler · 08 | stage_plan + stage_scene + root | IN PROGRESS | 手工包软件基础可并行；完整退出需 18 | v2 已接直道/广场/缓弯/低桥、固定拱门和同轨道预告；121 项相关测试、5 项场景补验、42 项 CPU 与最终 16 张 GPU 图通过，原拱门遮挡已修并保留失败图；完整计划来源诊断另补 66 项 Lab 测试、五包 30 条真实 CLI、Clippy / 格式与 466 项冻结输入构建通过，见[观察](../testdata/synthetic/stage-plan-observations-20261008.json)；明确 Stage 1 / 2 的只读观看已交付，完整自动音乐结构 / 编排、设备性能与真人可读性仍待完成 |
| 20 | 内容编辑器与 Replay 诊断 · 09 | editor_core + section_qa + root | IN PROGRESS | 现有包与 Replay 契约可先行；完整体验依赖 18/19 | 原生波形工作台、精确编辑、保真导出和 Replay JSONL 已接通；2026-10-05 重跑 35 项测试、10 条 CLI 命令和 8 张原生 GPU 图通过，详情裁切和底部滚动已补验；历史 GUI 编辑 / 恢复独立记录，本批只读 Replay 图形诊断已补两尺寸 10 张原生 GPU 图、76 条记录浏览与详情底部滚动，候选、计时、试听和完整退出继续推进；新增 Labels 隐藏提示编辑已有 57 项正式 Lab 测试和冻结构建，分版原生补验和最终法语小窗口指定目检通过，OS IME / 设备 / 真人盲标另验；原生 beat / raw downbeat 只读消费者及分版软件验证已交付，图例视觉失败 / 定向修复保留，见[工作台](../docs/native-beat-candidate.md#原生候选工作台)，音乐准入与设备 / 真人边界保持；[MusicTruth](../docs/music-truth.md) 四 CLI 人工事件软件入口另补 20 条真实控制，coverage 交集精确帧分歧与音频身份保留，真人音乐参考 / 置信校准仍待完成 |
| 21 | QUIC 会话、时钟同步与可靠历史 · 10 | stage_curve_plan + net_wire + net_tls + root | IN PROGRESS | 20 | 预装同包的真实 QUIC 历史会话已接 lab；11 项相关测试、13 项 loopback 场景 / 30 命令通过，断线前缀和应用 Ack 有实际证据；有界原字节接收已补 25 项单元测试、16 条 loopback 命令和 6 类恶意传输通过，进程 ClockSync / 未来起点已补 13 项 net 测试、16 条命令和 11 项时钟故障 / 资源回归通过，单局音频和生产接线已补 134 项定向测试、7 组 live loopback、两个原生游戏进程和 16 条 headless 回归；真实双方 71 条事实 / 5 个事件相同，本批 169 项测试及两个原生进程连续两局通过，每局新 epoch、独立记录和相同权威结果；随后故障新局和同 epoch 原音源软件恢复已有证据，长期漂移、容量与设备验收继续保留 |
| 22 | 网络模拟与故障验证 · 11 | section_qa + root | IN PROGRESS | 21 | loopback 实际进程及受控 peer 的延迟水位、中途断线、身份/TLS/Ack 拒绝已验证；真实 UDP 黑洞 / deadline 软件模型已补证据；两机、非对称路径 / 漂移完整矩阵与实际声学同步继续保留 |
| 23 | 两台真实机器验收 · 11 | 待分配 + 设备测试 | NOT RUN | 11/12/21/22 | 两端 Replay、真实音频偏移、伙伴反馈延迟、重启/断线与双人体验记录 |
| 24 | 四目标发行与 V1 加固 · 12 | root | IN PROGRESS | 01/14–23/26–29 | 固定 `8cdda9d` 同 ref 四优化包及十二项 Lab smoke 软件 PASS；干净机器、GPU/音频/手柄、帧时间/underrun/内存与实际 tag Release 尚未验收 |
| 25 | 品牌资产、启动动画与发行图标 · 用户新增需求 | 品牌线程 + root | IN PROGRESS | 独立资产和模块交付、08/09 | 品牌 67962a3 已在固定源码 2329c19 完成生产软件补验，59 项测试、Clippy、格式、依赖边界、构建和 GPU 截图视觉检查通过，详见验证策略；输入仍等待 Complete 与新确认；真实音画同步、物理输入和平台图标显示未验收 |
| 26 | 设置草稿、应用/取消、持久化与显示预览 · 02a | root | IN PROGRESS | 08/09 的菜单与输入门控 | 软件实现已完成，67 项 workspace 测试、Clippy、构建和 GPU 设置截图通过；覆盖原子保存、失败保留、15 秒预览回退、分辨率列表及键盘/手柄释放屏障；真实设备完整操作仍待验收 |
| 27 | 首批 13 个语言变体的界面 · 02a | 国际化 agents，root 集成 | IN PROGRESS | 26 的设置流程 | 软件实现已完成，13 × 91 条文案、系统识别、地区变体、原子保存和即时切换已接入；75 项 workspace 测试、Clippy、构建及 29 张 GPU 图检查通过，Noto Sans 与旗帜已在 b02dc42 提交；CJK 词边界诊断保留并记录上游修复路径；Windows/Linux 实际输入导航与母语使用者验收仍为 NOT RUN |
| 28 | 画质预设、独立效果与帧率/VSync · 02a | quality_settings / quality_render / quality_pacing，root 集成 | IN PROGRESS | 26 的设置流程 | 已实现低中高/自定义、五项效果、最高刷新率档位、独立 VSync 与生产 CPU 门控；81 项 workspace 测试、Clippy 与构建通过，覆盖联合事务、跨屏归一化和有效历史窗内的 Session/Replay 消费批次一致性；46 张 GPU 离屏图已逐张检查通过，实际呈现帧率/VSync 与平台设备验收仍为 NOT RUN |
| 29 | 窗口、无边框全屏与渲染尺寸 · 02a | display_settings + root | IN PROGRESS | 26 的设置与显示预览流程 | 已接请求/回读、原生 UI、游戏与设置菜单共用结构化滚动、Windows 工作区与 X11 保守交集、环境变化时一次容纳；92 项测试、Clippy、Linux 构建及 Linux 上的 Windows 目标编译检查通过；已修复未编辑显示草稿与外部窗口变化的同步，极小 Ready 菜单越界已修复，28 张菜单与设置 GPU 图逐张检查通过，真实 WM/DPI/跨屏和 Windows 运行仍为 NOT RUN |
| 30 | UI、HUD 与双人输入体验 · 02b | UI/input agents + root | IN PROGRESS | 02/02a 的输入与设置契约 | 独立菜单主控、混合输入换席、真实结果/故障、160 ms 焦点反馈和阶段等待标记已实现；本批完整检查含 89 项 runtime 测试，四档画质与小窗口 15 项必要反馈图通过，GPU 历史失败及补验保留；设备和真人验收继续见 02b |

当前软件范围 02–10 已汇合并验证，确定性规则与 Replay 已提交为 `378f95b`；完整阶段 01/02 的退出仍等待 11/12；root 按里程碑提交主线程实现，保留品牌线程后续改动，代码修订需补充对应验证

提交 `7c37972` 的本机 Linux x86_64 release 构建、真实 tar.gz 打包、独立解包 CLI 与 GPU 离屏启动均为 PASS，命令、产物哈希和截图见 [release 验证记录](../docs/testing.md#原生-linux-release-证据)；产物要求 `GLIBC_2.44`，不代表 Ubuntu 24.04 兼容，Windows/ARM64、音频/输入设备与真人验收仍为 NOT RUN，该段为当时结果，01 的后续原生构建结果见上表，24 的完整设备发行门槛仍未完成

提交 `5949c13` 另在 Ubuntu 24.04 x86-64 容器完成 92 项测试、格式/Clippy/边界检查、原 release 配置构建和 62 文件包的独立解包检查，`--help` 与合成 Replay CLI 为 PASS，最高要求 `GLIBC_2.39` 且动态库全部解析；源码、镜像和产物身份见 [Ubuntu 基线验证](../docs/testing.md#ubuntu-2404-容器发行基线)。远端 [首次轻量 CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/36963977703) 的 PASS 仅适用于底座 `b2950cc`，该段记录的容器源码与后续远端构建分别取证，未新增实际桌面、设备、真人验收

下一步验收顺序是软件链路 → 11 硬件 loopback 与输入计时 → 12 平台和手柄 → 13 真人体验；09/10 的可逆实现和自动检查可与设备准备并行，01 远端构建可独立执行

26–29 的完整需求与验收见 [02a 设置、国际化与显示](02a-runtime-settings.md)，软件工作可与设备准备并行，平台最终验收仍需实际证据；2026-10-03 用户已要求启动 UI 重设计，正式任务见 [02b](02b-runtime-ui.md)

2026-10-03 用户批准继续后续游戏开发，已从设置转入 05 源解码、重采样和编码准入；media/MIR/stage/editor/net 按实际软件契约逐步加入，完整阶段退出仍保留设备与真人证据，模拟报告不替代这些结果

同日按用户新要求，当前优先推进 30 的可操作 UI 与双手柄、14 的游戏场景及合作反馈，内容管线实验保留，后续工作依旧按依赖继续

2026-10-07 · 10 资源接收：25 项定向单元测试、16 条真实 loopback 命令和 6 类恶意传输检查通过；协议 v2 在完整四对象原字节校验与包身份成功后才 Ready，源 manifest 快照绑定与失败 staging 清理已补齐，持久化观察及 NOT RUN 边界见 [验证策略](../docs/testing.md#quic-四对象资源接收)。下一批继续 ClockSync / 未来 ScheduleStart，游戏窗口生产接线仍待后续

2026-10-07 · 10 进程时钟与预约起点：默认假定漂移范围内的四时间戳 / 不对称路径区间已由协议消费，有限重试、可靠确认、准备期限与软件迟到分别记录；实际 5 组时钟故障和升级后的 6 类资源拒绝回归通过。下一批接生产单玩家输入 / 水位、音频与伙伴反馈，真实设备 / 双机 / 真人证据继续保留

2026-10-07 · 10 实时生产接线：protocol v4 worker 连接完整资源校验、后台 PCM 装载、显式 Ready、双方 Armed、未来 Kira 播放、单玩家水位、即时本地 / 已确认共享反馈与 FinishAck。取消前 Ready 关闭未刷出的实际失败已修复并回归，窗口退出等待 worker 清理；两个原生进程完成并正常退出，受控输入是软件注入，物理设备与双机未验收。固定源码 / 产物 / 原始日志身份与历史失败见 [验证策略](../docs/testing.md#quic-实时游戏与原生软件接线)，完整 10 保持 IN PROGRESS

2026-10-07 · 09 候选工作台：`workbench-candidates` 共用 Anchor 报告来源与重新编译校验，逐项显示原始 index、未知评分、拒绝原因和阻挡关系；正式 lab 28 项测试、Clippy / 格式 / 依赖边界通过，三组源码副本原生窗口 11 PNG 目检通过，修正前失败和精确证据范围见[观察记录](../testdata/synthetic/candidates-workbench-observations-20261007.json)。只读查看已完成，试听与设备计时关联继续保留

2026-10-07 · 06 模型研究：Beat This! small0 与 BSD DBN 已完成源 / 严格 canonical 回读各 120 行 CPU 数值对照，官方与 ORT 数值通过；变速、6/8 和摇摆质量继续 FAIL，`production_admission=false`，完整前处理、原生加载、分析能力与独立人工标签待后续，见[研究摘要](../tools/beat-model-probe/results-2026-10-07.json)

2026-10-07 · 08 / 10 舞台身份与故障新局：明确 Stage 1 / 2 编译、Replay v2 实际版本及 protocol v5 的完整身份已接线；184 项 Stage 相关测试、12 个真实 wire 拒绝及 3 个完整 PCM 控制通过。故障后新邀请恢复另取得 127 项 runtime、9 组 worker 和 3 组原生双轮证据，固定构建 / 文案范围见[Stage 观察](../testdata/synthetic/stage-version-observations-20261007.json)与[网络观察](../testdata/synthetic/network-reentry-observations-20261007.json)。旧前缀不补齐，旧 Replay 保留，完整视觉观看、同 epoch 续演及真实双机另推进

2026-10-07 · 生产源导入与工作台试听：原始音频经唯一 q10 编码进入手工四对象，10 条成功 CLI、5 条预期拒绝、两张生产 loader 图通过；工作台三模式复用 PCM 并分开显示原始 / 音频 / 请求坐标，17 项最终窄测、Kira MockBackend 和四张构造 UI 图通过。输出设备、计时与听感另验，下一批推进实际视觉 Replay、性能测量及完整曲库 / 内容能力

2026-10-07 · 工作台 CPAL 接线增补：两个尺寸实际 callback 启动 / 暂停 / seek / 恢复 / 失焦 / 停止与一个实际初始化失败共三 case 通过，七张实际图 agent / 主线程逐张检查；只读原始事实保持，物理输入与声学验收独立

2026-10-07 · 原生只读视觉 Replay 已交付：`--watch-replay` 使用明确 Stage 1 / 2、原事实顺序与实际 Kira acknowledged cursor，完整开场后明确确认才播放；暂停、恢复、原 epoch 重启及返回菜单保持原包 / Replay，完整与 partial 不补造历史。133 项 runtime 测试、最终标题窄测、Clippy / 格式通过，四个实际窗口 case 与最终三张静态标题图分别绑定冻结二进制；证据和历史失败见[观看记录](../testdata/synthetic/visual-replay-observations-20261007.json)，历史 shader、设备计时及真人验收另计

2026-10-07 · 02b 小窗口标签：两个身份提示改为独立区域并随窗口高度定位；实际字体布局与确切静态帧软件验证通过，原重叠 / 遮脸失败保留，完整设备体验未据此退出

2026-10-07 · AudioFlux MIT 原生 C onset 研究完成：内置 FFT / 原 Matcher / 12 项边界控制及 29 原 PCM 恢复通过，原 34 项质量仍 2 PASS / 28 FAIL / 1 不支持 / 3 不评分；宿主同二进制同参数 sanitizer 无诊断，原 sandbox 失败保留。无 Python 产品后端或 GPL 分析依赖，未接入生产，见[观察记录](../testdata/synthetic/native-onset-observations-20261007.json)

2026-10-07 · Ready 曲库生产选择：已制作四对象包经后台完整验证才换歌，取消 / 三类拒绝保留原歌曲与 Replay，成功后停 Ready 等新确认；5 项行为检查、焦点反馈窄测、Clippy / 格式及六项原生 case 通过，42 张指定范围截图通过，旧小窗口视觉失败保留。自动 MIR 和游戏内原始源导入继续推进，见[曲库验证](../docs/testing.md#生产曲库选择与小窗口反馈)

2026-10-07 · BTT MIT 原生 tempo 研究：native / sanitizer 各 16 项边界控制及原 40 条 source / strict canonical 软件观察完成，40 对输出逐字节一致且无 sanitizer 诊断；变速误差与 13/20 编码前后曲线变化完整保留，质量 UNSCORED、生产分析未准入，见[方法与边界](../docs/mir-onset-probe.md#btt-原生-tempo-研究)

2026-10-08 · 10 同 epoch 原音源软件续演：27 项 net、另 2 项实际 host loopback、160 项 runtime、3 项 sampler、Clippy / 格式和冻结 game 构建通过；实际 UDP 黑洞 / reliable deadline 的整数源模型与实际 Kira 双进程主动维护分别 PASS，原 epoch / source generation / source_id / 完整历史保持，恢复期负向 Hit 被过滤。最终双方各 3393 条事实 / 17 个 core 事件及权威 Replay 一致，完整命令 / PID / exit 清单和四 PID / 进程组退出已核对，见[恢复观察](../testdata/synthetic/same-epoch-recovery-observations-20261008.json)

原 50ms guard FAIL、QA 开场 flag 误断言和首次 sampler 运行的命令清单 INCOMPLETE 保留，最终 manifest 修正后重新实际运行才计 PASS；四张 Running / Finished 图的角色、状态、时钟和标签由 agent / 主线程逐张指定范围检查，长 CID 溢出仍归 02b，Recovering 图未取得。工作包 21 / 22 保持 IN PROGRESS，下一批继续共享生产源导入、优化 release 性能矩阵和原生许可宽松分析；无 GPL 或 Python 产品后端，实体输入 / DAC / 长期漂移 / 双机 / 真人验收保持 NOT RUN

2026-10-08 · 共享源导入与游戏 CLI 已验证：3 项 authored、30 项 lab、19 条 lab CLI 和 6 项 game 拒绝通过；445 项真实冻结输入与源码副本一致，新 `--import-authored` 从原创源实际创建包、完整开场后停 Ready 并等新确认，原 Kira 播放和 13 次双人软件输入完成，game / wrapper exit 0。观察见[导入记录](../testdata/synthetic/authored-game-import-observations-20261008.json)，完整自动 MIR 与 Ready 文件选择仍继续推进，下一批为新优化 release 的性能矩阵；恢复提交 `2a5fe59` 的 [Lightweight CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37655665012)已通过，设备 / 声学 / 真人验收仍 NOT RUN

2026-10-08 · 优化 release 原生性能观察：445 项冻结输入一致，实际 Linux / CPAL / AMD Vulkan 原环境矩阵 13 VALID、1 Wayland VSync 超时；最慢有效 limited60 两次复测 VALID，另一个隔离 X11 VSync 补测 VALID，未覆盖原失败。无限制组 Running p95 为 1.820–3.585ms，limited60 三次为 16.732 / 16.734 / 16.731ms，X11 补测 p95 / p99 为 33.320 / 33.673ms；记录的是 main-update 间隔与含探针缓存的 RSS，不是呈现 FPS、GPU 耗时或跨硬件预算。见[性能记录](../docs/runtime-performance.md)，共享源导入提交 `a617be3` 的 [Lightweight CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37658605442)已通过；继续 CID 换行、MIR 能力与完整内容接线，设备 / 声学 / 真人验收保持 NOT RUN

2026-10-08 · 完整 CID 行换行：共用 RowPrefix 使用 Bevy WordOrCharacter 后备，完整身份文本保留；实际 Noto 字体布局检查覆盖 zh-CN 1280×800、en-US 640×480 和 de 2×DPI，同一文本切旧 WordBoundary 必须越界，恢复新值必须重新通过。1 项窄测、runtime 全目标 Clippy 和格式通过，见[换行记录](../testdata/synthetic/menu-cid-wrap-observations-20261008.json)；本批没有新的联网 CID GPU 图或设备 / 真人可读性结论

2026-10-08 · analysis v2 软件交付：schema / media / lab 共 80 项检查、runtime 内容 8 项、真实 v2 字节拒绝窄测、四包全目标 Clippy、格式和冻结 lab 构建通过。14 条真实 CLI 保留旧 v1 四对象与原 Replay 身份，新 final Ogg / 原 WAV 制作默认 v2，音频 / chart 字节不变，analysis / hash 按版本变化；两次旧 Replay 对新身份的拒绝符合预期，见[能力格式观察](../testdata/synthetic/analysis-v2-observations-20261008.json)。完整 MIR 算法仍未准入，继续 Ready 手工源导入、许可宽松原生分析与内容路线；CID 修复 `9018cc1` 的 [Lightweight CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37668513677)已通过

2026-10-08 · Ready 手工源导入已接通：正常无参开场后从曲库选择配对源与 authoring，明确确认才在后台制作四对象并完整加载，成功后停 Ready 等新 Start；164 项 runtime 测试、真实 worker 窄测、Clippy / 格式 / 架构边界、13 语言新文案字形检查通过。固定 debug 图的七项源导入和六项原曲库回归、26 个实际 PID / 进程组退出通过，22 / 42 张截图仅五张源导入图有指定范围目检；未完成的忙操作子项和设备 / 声学 / 真人验收保留 NOT RUN，见[Ready 导入观察](../testdata/synthetic/ready-source-import-observations-20261008.json)。下一步继续许可宽松原生分析、独立标注与完整 MIR / Anchor / 内容编排

2026-10-08 · 长曲与软件计时：`9207073` 已将 Hit / 共振范围查询改为 std 有序索引，原 9 项 core 检查及逐步差分一致；[Lightweight CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37685653393)已通过，开发测试用时不作游戏帧率结论。后续显式本机 timing sidecar 保留原 Replay / CSV、当次 mapping anchor 和独立 source / callback 历史，core 9 / replay 11 / runtime 174 / lab 32 / xtask 4 检查、Clippy / 格式 / 边界、13 语言新字形通过；447 项冻结输入、32 条 CLI、两组原生游戏及一个成功的工作台详情窗口取得软件证据，首次 GUI QA 焦点失败和三图有限目检范围保留，见[计时记录](../testdata/synthetic/timing-sidecar-observations-20261008.json)。继续许可宽松原生 beat 候选、独立人工标签入口和完整内容编排，实验质量 FAIL 不升级为准入，物理设备 / 声学 / 真人与其他三目标 native 计时仍 NOT RUN

2026-10-08 · 独立人工标签软件交付：`label-source`、`import-labels` 和 `compare-labels` 已接正式 lab，40 项 Lab 测试、近上限 1 项重复窄测、最终 Clippy 与 449 项冻结输入构建通过，固定 64 秒包的 33 项真实 CLI 控制通过。来源四对象、原标签及已有输出保持，448 条报告左右原记录完整保留；首次测试断言 Clippy lint 和复查证据均保留，见[独立标签](../docs/independent-labels.md)。有谱面上下文的试听加外部 JSON 是当前人工记录路径，GUI 标签编辑 / 隐藏提示盲标未实现，实际真人标签、听感 / 可玩性、confidence 校准、跨平台 CLI 和加载至写出间完整来源替换竞态仍 NOT_RUN

2026-10-08 · 原生 beat 候选软件交付：Lab 显式实验入口复用最终 canonical 音频与手工四对象事务，固定 CPU SDK / 模型，候选 confidence=None，不自动 Anchor；media 46 / xtask 5、native 3 项复跑、最终 Clippy 和 463 项冻结构建通过，实际 12 条成功命令、四组同 spect 数值对照与 13 项事务 / 资源控制通过，手工导入四对象字节保持，初 op_ref lint 失败保留，见[原生候选](../docs/native-beat-candidate.md)。原 wholeSpect 19 PASS / 9 FAIL（整体 FAIL）与音乐质量 FAIL 不升级为准入；四目标 workflow 已接资源和解包 smoke，本次四目标 native run / 新 tag Release、十分钟成本 / 取消及设备 / 真人仍 NOT_RUN，当前目标继续 active

2026-10-08 · 独立 Labels 编辑软件：`workbench-labels` 已接正式 Lab，隐藏作者 / 候选 / section 提示，共用 final stereo PCM 波形试听；人工点 / 半开区间、三态判断、原生 EditableText / IME 软件门控、Apply / Cancel dirty 草稿及新侧车保存 / worker join 已实现。正式 57 项 Lab 测试、Clippy、465 项冻结构建和独立静态审阅通过，13 locale × 39 条文案的 Noto 覆盖第二轮通过，首轮 ≤ 字形失败保留；见[独立标签](../docs/independent-labels.md#原生-labels-工作台)。旧 CLI 实绩保持，原生 GPU / 窗口补验本候选编写时 NOT_RUN，OS IME / 物理输入 / 母语 / 真人盲标及 confidence 校准另验，该 UI 批次当时标签采用未接线，当前目标继续 active

2026-10-08 · Labels 原生补验收尾：首轮 4 项软件 case / 14 PNG 与小窗口视觉 FAIL 保留，布局修复后 5 项 / 17 PNG、11 张指定目检；法语编辑路径补验及最终清理文案后的法语窄测各 1 项 / 4 PNG 全目检，总 11 项 case / 39 PNG 按冻结版分别记录。最终 Lab 57 项、Clippy / 格式、465 项冻结输入及 13 × 31 条文案字形通过，已删除每语言 8 条未用文案，旧 39 条覆盖实绩保留；详见[Labels 验证](../docs/independent-labels.md#原生-labels-工作台)。最终仅法语小窗口补验，OS IME、物理输入、声学试听 / 计时、母语 / 真人盲标及校准仍 NOT_RUN，该 UI 批次当时标签采用未接线，当前目标 active

## 当前原生候选发行补验

工作包 24 的软件发行记录新增固定 `0930d17` 三目标 PASS：Windows x64、Windows ARM64、Linux x64 的优化包与十条解包 Lab 命令通过，实际字体 QA 排除、SDK / provider / 模型字节与许可来源匹配通过；Linux ARM64 Build FAIL，runner 失联，最终日志不可用，实际 Cargo exit / 最后 crate 为 `BLOCKED_LOG_AVAILABILITY`，其包与软件命令 NOT RUN，完整四目标准入未完成，见 [清洁包矩阵](../testdata/synthetic/native-beat-release-observations-20261008.json)

[原始矩阵](../testdata/synthetic/native-beat-release-original-observations-20261008.json) 三项 `verify.py` 包排除 FAIL 及 ARM exit 143 原因未知保持；仅 ARM64 后续采用 `--jobs 1` / GNU time 诊断，不改 fat LTO 或 timeout，不宣称 OOM 已证实或已修复，静态软件检查与六项 shell stub 窄控已通过，后续固定 ref 的实际构建单独记录，本冻结矩阵时 GNU time 诊断 GHA / RSS 为 NOT RUN，失联仍可能导致记录缺失

本批固定版本不覆盖后续 Labels GUI / 采用提交，Game GUI、真实设备 / 真人、干净 Windows VC、模型质量、reference logits/q 与实际 tag Release 继续独立取证，工作包 24 的完整 V1 退出状态保持

2026-10-08 · Linux ARM64 发行诊断补验：固定 `d3b2648`、通过 CI 后唯一运行 [37708701226](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37708701226) 成功，独立 TAR、38 份源码许可 / 四份 SDK notices、597 行台账和十项解包 Lab 命令核验通过；实际 GNU time wall 29:35.45、最大 RSS 11386900 kbytes、command / tee exit 0，构建 QA 诊断单独保存且不入产品，见 [ARM 观察](../testdata/synthetic/native-beat-arm-release-observations-20261008.json)

旧两次 ARM 失败和三包 QA 排除 FAIL 保留，本次成功不解释其根因；另三平台仍是 `0930d17`，本 ref 完整四目标未重跑，右侧 beat 数量 114 与历史 Linux x64 113 保留为独立实际值，数值 / 音乐质量、Game GUI / 真实设备 / 真人、干净 Windows VC 和实际 tag Release 门槛仍由各自证据关闭，工作包 24 的完整 V1 退出状态保持

2026-10-08 · 同提交四目标发行软件收尾：固定 `8cdda9d` 通过 CI 后，Windows / Linux × x86-64 / ARM64 四个原生优化 Game / Lab 包及解包软件检查通过，48 条实际 Lab 命令为 44 条成功和 4 条预期缺失模型拒绝，八份左右声道 tempo 报告共 192000 行完成核验；包内源码、资源、许可台账与字体 QA 排除按该提交身份匹配，见[同提交四目标观察](../testdata/synthetic/release-four-target-8cdda9d-observations-20261008.json)及[原始证据索引](../testdata/synthetic/release-four-target-8cdda9d-observations-20261008-raw-index.json)

原 `07830f8` 四目标 FAIL 单独保留：Windows 两架构因未声明的 `random()` 构建失败，Linux 两架构完成构建后在首次实验导入的 Ctrl+C 注册处失败，tempo 和后续命令未运行，见[原 078 四目标失败观察](../testdata/synthetic/release-four-target-07830f8-observations-20261008.json)及[原始证据索引](../testdata/synthetic/release-four-target-07830f8-observations-20261008-raw-index.json)；更早 `0930d17` 三目标与 `d3b2648` 单 ARM 的历史矩阵保持各自源码范围

Linux ARM64 本次 `--jobs 1` 构建的 GNU time wall 为 13:03.42、最大 RSS 11453596 kbytes，属于单次 runner 环境观察，不作为预算或旧失败根因。此矩阵不验收 Game GUI、干净机器 / Windows VC 前置、实体输入 / 音频、真人音乐参考或实际 tag Release；旧 wholeSpect 19 PASS / 9 FAIL（整体 FAIL）与音乐质量 FAIL 保留，tempo 的 beat_unit / meter / confidence 仍为 None、质量 UNSCORED、production_admission=false；后续内容 / GUI 软件与设备 / 真人任务继续按各自证据推进，完整 V1 退出和实际 tag Release 未关闭
