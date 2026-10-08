# 当前执行目标

2026-10-09 · [Stage 3 上下文编排](../testdata/synthetic/stage-context-observations-20261009.json)已完成软件接线：重复关系驱动一致环境配色，实测 RMS 驱动微光，旧 Stage 1 / 2 几何与 Replay 保持；295 项初测、98 条 CPU / CLI 和四张离屏 GPU 图通过限定检查。继续双音源相位校正、MIR 质量、完整内容路线与最终四目标验收，目标 active

2026-10-09 · [未标注歌曲推断软件](../testdata/synthetic/anchor-unlabelled-inference-observations-20261009.json)已接线：冻结 bins / Choice 推断、只读工作台、明确采用与游戏 Ready 通过；Lab100、34 项 CLI、5 项公共 CPU 控制和单窗口 Kira 试听取得软件证据。继续舞台上下文编排、双音源相位校正、MIR 质量和最终四目标验证，目标 active

2026-10-08 · [实时进程时钟维护软件](../testdata/synthetic/live-clock-maintenance-observations-20261008.json)已接生产：protocol / ALPN v7 在 Running 期间每秒有界刷新原四时间戳，typed Stale 进入原一次续演，可靠 FIFO 与新 epoch 重建保持；net31、真实 loopback3、runtime175、Clippy / 格式 / 边界与当前 Game / Lab 构建通过。正常及真实 UDP 黑洞恢复完成双方相同权威 Replay，黑洞以真实样本超过原 2 秒有效期触发，未注入恢复请求；当前目标 active，继续双音源长期相位校正、音乐准入与最终四目标加固，实际 Kira 丢包 / 设备 / 双机及真人另验

2026-10-08 · [实验校准与漂移消费软件](../testdata/synthetic/calibration-drift-observations-20261008.json)已完成：Media86 / Lab96、必要检查与当前 Lab 构建通过，33 条校准 CLI（24 成功 / 9 预期拒绝）验证 train / Choice / evaluate / v2 明确采用及原 None / native / 音频 / analysis / v1 保真；五条正确漂移 CLI 保留原报告和未知来源语义，首轮 lint、侧车误配与 QA setup 失败保留。音乐策略准入、未标签推断、v2 图形消费者、双源长期校正及新四目标发行仍继续推进，目标 active

更新：2026-10-08，用户要求继续完成软件开发与验证，完成后结束自动目标并等待用户真实验收；有界 QUIC 四对象接收、网络时钟和单局实时游戏已取得软件证据，正常完成后的同进程新局与只读 Replay 图形诊断也已取得软件证据，随后继续生产音频和内容路线

## 总目标

完成 CoCoBeat 的 Windows/Linux × x86-64/ARM64 软件交付：由现有本地双人原型继续完成标准音频、曲库、SongPackage、MIR、Anchor、舞台、编辑与联网，并取得适合各功能的软件验证与构建证据；本轮自动目标完成后停下，设备、双机、听感、母语与真人体验交由用户真实验收

每个里程碑交付可运行实现、对应验证和英文 Conventional Commit；优先分工并行，主线程统一共享接口。当前 main 已获批准推送，四目标手动构建以固定源码取证；保留品牌线程改动。依赖使用最新稳定版和主版本范围，由 Cargo.lock 固定实际解析结果

用户于 2026-10-07 明确允许在各模块采用适合需求的成熟库，不追求纯 Rust；生产音频选择静态内嵌 vorbis_rs，软件准入后使用唯一编码路径，许可证、跨目标构建、数据完整性与故障处理继续核验

同日用户进一步明确：生产音乐分析不接受 GPL 和 Python 部署，继续筛选许可宽松的原生库 / 模型；已有 Python 研究工具与失败结果是 QA 历史，不作为产品分析后端

此前优先交付玩家可见的界面、场景与反馈更新，UI 重设计正式进入 [02b](02b-runtime-ui.md)，与 [04](04-rain-neon-art.md) 并行；用户已确认柔和玩具主体、雨夜空间层次、关键同步强反馈的方向，并要求适配双手柄、键盘＋手柄以及独立于玩家编号的菜单主次控制。先解决菜单遮挡角色、操作层级与场景可读性；音频和 MIR 已有实验保留，后续内容管线继续按依赖推进

2026-10-03 首批 UI 与输入软件实现已落地：独立菜单主控、显式接管、两种键盘＋手柄分配和双手柄门控已有自动检查；雨夜角色、街道与五种反馈的静态样例通过。后续继续完成 02b 的结果/错误层级与连续动效、04 的等级反馈和性能证据，并按依赖推进内容管线；设备和真人验收未由这些软件结果替代

同日第二批已补真实本局统计、阶段菜单、Replay 重试与隐式保存失败保留、设置期间故障提示和返回焦点；Precise / Good 区分亮度并共用运动时长，取得 240 张真实 core 驱动的连续 GPU 帧。菜单与连续体验仍结合设备/真人继续迭代，软件工作并行转向编码数值域、宽频瞬态损失和保留频谱证据的 MIR 候选，不能以软件样本替代完整项目退出

混合输入复查进一步补齐菜单解除手柄分配、无需拔插的双向换席和断连后的操作焦点保留，未加入手柄在演奏中不触发保存；菜单主控仍独立于 P1/P2。86 项 runtime 测试与 13 语言差异字形检查通过，真实设备验收继续保留

## 已有基础

- 工程底座、确定性规则、Replay、64 秒原创内容、本地双人运行时及品牌生产接线已有软件检查与 GPU 呈现证据，见 [验证策略](../docs/testing.md)
- [02a](02a-runtime-settings.md) 的设置草稿、原子保存、显示预览、13 个语言变体、Noto Sans、旗帜、画质、帧率与小窗口滚动已实现；加入源解码、重采样与十分钟 Replay 后的完整软件基线为 100 项测试，原生 WM/DPI/跨屏、物理设备与母语真人校对仍待验收
- Linux 两架构在 `4c63dcc`、Windows 两架构在换行修复 `34eaf5b` 已通过原生发行构建及下载包静态核验；`75b82f4` 的 [Lightweight CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37090636695) 已通过 29 项测试并恢复旧缓存，同提交音频候选的四平台原生软件验证通过；各结果只适用于记录的源码版本，见 [构建记录](../docs/build-release.md) 与 [音频候选](../docs/canonical-audio-probe.md)
- 真实 Kira 后端的 30 秒游标记录已取得，物理延迟、loopback、平台手柄与真人双人体验仍未验收；软件时钟、MockBackend、构建和截图不替代这些证据

## 当前并行里程碑

| 工作 | 下一项交付 | 完成依据 |
|---|---|---|
| 02b UI 与 HUD | 完成/故障、混合输入换席与真实状态反馈已补齐，继续可读性和设备/真人迭代 | 本批完整检查含 89 项 runtime 测试，四档画质与小窗口的 15 项必要反馈图通过；历史失败及补验保留，真实双手柄与混合设备单独验收 |
| 04 场景与特效 | 角色、雨夜空间和两档 Anchor 反馈已落地，继续画质与性能预算 | 240 帧实际 core 反馈、17 个关键帧目检及新增 15 项静态画质反馈矩阵通过；固定模拟帧率不代表实测 FPS，真人可读性另验 |
| tag 自动发布 | `Vx.y.z` 与 Cargo 版本一致后，CI → 四目标构建 → 四份压缩包发布 | 工作流语法与本地门控检查通过；实际 tag 发布需按对应运行取证 |
| 四目标构建已通过 | 固定 `8cdda9d` 四原生优化包与十二项 Lab smoke 已通过，继续实际设备发行验收 | 同 ref 四目标、48 条实际 Lab 命令与八份 tempo 报告完成软件核验；干净游戏设备、GUI / 输入 / 音频和实际 tag Release 另验 |
| 05 源音频导入与重采样 | 源解码、48 kHz High 重采样及严格最终读回已由 lab 消费 | 本批 Linux 25 项 media 测试通过，既有 27 项音频 staging CLI 检查保留，既有 21 项严格读回证据保留；原 `75b82f4` 四平台各 6 项 media 测试独立保留，新入口跨平台及质量听感仍另验 |
| 05 编码准入 | 静态内嵌 vorbis_rs 本机与四目标原生软件准入已交付，继续完整曲库与听感 | 四个冻结源的 finite-only/fullband 组合已双路完整回读，核证书与条件式算术预算已核对；残余失真及局部退步保留，旧候选 guard 不变；新编码路径本机 39 测试、14 完整回读、38 质量控制和修补后 11 Sanitizer 通过；四目标原生各 14 项控制通过，本机 695 明确前滚窗口通过；其他平台 seek、曲库与听感另验 |
| 05 SongPackage | 四对象、手工创作、播放、段落表现、Ready 曲库与配对源导入已接通，继续完整内容编译 | 手工 SectionCue 已驱动字幕和预告门，混合语言使用内置 Noto 回退；曲库另补 5 项行为检查、焦点反馈窄测、六项原生 case 与 42 图指定范围验证；103 项 runtime 测试、13 项 CLI 与 10 张静态 GPU 图通过，完整 MIR / Anchor / Stage 能力仍待实现 |
| 08 舞台软件基础 | v3 完整上下文编排已交付，继续音乐质量与性能测量 | 121 项相关测试、修图后的 5 项场景补验、42 项 CPU 用例及最终 16 张 GPU 图通过；拱门遮挡已修，明确 Stage 1 / 2 的只读视觉观看随后已交付，新增 66 项 Lab 测试、五包 30 条真实 CLI、Clippy / 格式与 466 项冻结输入构建通过，完整计划见[观察](../testdata/synthetic/stage-plan-observations-20261008.json)；完整自动结构、设备性能与真人可读性仍待完成 |
| 06 MIR 基准前置 | 来源清单、独立标签侧车及显式原生 beat 候选已接通，继续完整 MIR / 人工审阅 | 旧质量 FAIL 与人工 pending 保留；固定 canonical CPU 候选的 12 条成功命令、四组同实际 spect 对照和 13 项事务 / 资源控制通过，confidence=None；原 wholeSpect 19 PASS / 9 FAIL（整体 FAIL）与音乐质量 FAIL 保留；Labels 隐藏提示编辑软件已实现，分版原生窗口软件补验与最终法语小窗口指定目检通过，OS IME / 设备、真人盲标与校准尚未完成；原生 beat / raw downbeat 只读消费者及分版软件验证已交付，图例视觉失败 / 定向修复保留，见[工作台](../docs/native-beat-candidate.md#原生候选工作台)，音乐准入与设备 / 真人边界保持；[MusicTruth](../docs/music-truth.md) 四 CLI 人工事件软件入口另补 20 条真实控制，coverage 交集精确帧分歧与音频身份保留，真人音乐参考 / 置信校准仍待完成；[40 项原生矩阵](../docs/native-beat-candidate.md#40-项导入与证据矩阵)完成导入 / reader 软件覆盖，原合成日程机械对照保留误报 / 漏检，旧 SDK 会话副产物证据保留，音乐质量 FAIL 与真人 / 校准仍未关闭；[Linux 实验导入取消](../docs/native-beat-candidate.md#ctrlc-取消原生候选导入)七项控制通过，真实 SDK 原 Err、同 PID fresh API 重试、600 秒前处理检查点零发布与初始化 guard 均按实测范围记录，Ready / GUI / Windows 和音乐质量另验；[原生候选覆盖对照](../docs/music-truth.md#对照原生候选)完成三流显式容差机械报告，7 项窄测 / 79 项 Lab 与 16 项实际 CLI 通过，构造参考不作为真人真值，音乐质量仍未准入；[原生 tempo 观察](../docs/native-tempo-candidate.md)完成固定绑定 / canonical 只读原值报告与单包两声道等价软件验证，UNSCORED / unit / meter / confidence 未知，首轮 QA FAIL 保留，固定 `8cdda9d` 四目标优化包及十二项 Lab smoke 已通过，可靠 TempoRegion 和音乐质量另验；[原生谱形诊断](../docs/native-structure-features.md)已交付固定窗 / 真实 EOF / 至多4个相似邻居的只读 JSON 与显式声道工作台，生产 media73 / Lab85 / i18n3及 Clippy / 格式 / 边界、9条生产 CLI 控制通过；[窗口验证](../docs/native-structure-features.md#只读结构工作台)两窗口 / 六 PNG 指定目检通过，首轮 QA 浮点读回 FAIL 保留；12条候选快照 CLI 与600秒成本另绑原快照，音乐语义未准入，新入口四目标原生 NOT_RUN |
| 07 Anchor 软件基础 | 独立提案与明确采用已接通，新增独立人工标签的显式点选择 CLI，继续标注策略与音乐审阅 | 实验提案历史 41 项相关测试、32 项真实包 CPU 检查保留；新标签采用软件验证通过，当前接线与记录见[独立标签采用](../docs/independent-labels.md#明确采用为-anchor)；构造分数与记录只验机制，生产置信度与可玩性另验 |
| 09 编辑软件基础 | 原生波形、精确编辑、保真导出与只读 Replay 图形诊断、候选证据、歌曲试听和显式软件计时已接通，继续独立标注及内容诊断 | 2026-10-05 重跑 35 项测试、10 条 CLI 命令和 8 张原生 GPU 图通过，详情换行与底部滚动已补验；历史 GUI 编辑 / 恢复另记，本批只读 Replay 波形、事实与配对诊断已补两尺寸 10 张原生 GPU 图，候选与试听随后已交付，观看随后已交付，计时及完整退出继续按新证据推进；新增 Labels 隐藏提示编辑已有 57 项正式 Lab 测试和冻结构建，分版原生补验和最终法语小窗口指定目检通过，OS IME / 设备 / 真人盲标另验；原生 beat / raw downbeat 只读消费者及分版软件验证已交付，图例视觉失败 / 定向修复保留，见[工作台](../docs/native-beat-candidate.md#原生候选工作台)，音乐准入与设备 / 真人边界保持；[MusicTruth](../docs/music-truth.md) 四 CLI 人工事件软件入口另补 20 条真实控制，coverage 交集精确帧分歧与音频身份保留，真人音乐参考 / 置信校准仍待完成 |
| 10 最小可靠网络会话 | 两进程预装或接收同包，邀请验证、可靠输入历史、同 core / Replay 和 FinishAck | 真实 loopback 的 13 场景 / 30 命令及 11 项相关测试通过，135 条完整事实与 16 个事件一致，断线/错误 Ack 保持真实状态；已补有界四对象接收，25 项单元测试、16 条 loopback 命令和 6 类恶意传输通过；已接进程 ClockSync / 未来起点，13 项 net 测试、16 条命令和 11 项故障 / 回归通过；单局已有 134 项定向测试、7 组 live loopback、两个原生游戏进程和 16 条 headless 回归；本批 169 项测试与两个原生进程连续两局通过，新邀请 / epoch / 历史均独立，故障后新轮次恢复已有软件证据，同 epoch 原音源软件续演已取得证据，长期漂移、容量与设备验收继续推进 |
| 计时、平台与真人验收（工作包 11–13） | 获取音频/输入、平台设备和双人行为证据 | 实际硬件与参与者记录；发现体验问题时回到规则和表现调整 |

按最新目标完成软件路线并分批提交，真实设备与真人验收不阻塞本轮自动目标结束；正式 V1 退出仍要求相应实际证据，未验收项继续标记 NOT RUN。05 稳定后按实际依赖推进 06 MIR → 07 Anchor → 08 舞台 → 09 编辑 → 10 QUIC 和软件发行加固，04 表现沿用现有设置和可读性约束

后续音频诊断的历史工作区入口为 `target/resample-l1-domain-20261003/summary.json`：四个固定采样率的真实 High 核及可达相位已核对，源峰值 0.997925 可产生约 2.751492 的重采样峰值；低幅度对照在原输入域仍有低 SNR，两入口产物一致，不能把损失只归因于放开范围检查。此后续目前仅工作区证据，需整理最小复现并追查编码质量；MIR 固定 Flux/HFC、谱变化过滤及近邻/慢起音/叠加控制已完成；分母下限候选保持近邻通过并恢复弱叠加，持续音旁瓣清零，但噪声及打击衰减仍有误报；固定时间背景门控已把额外峰从 269 减至 4，但漏检从 6 增至 16，失败诊断后停止继续堆叠单分数阈值，转向原创曲目来源清单与独立人工审阅，原始、时间适配与全部频谱候选结果保留

并行宽频诊断已冻结在 `target/codec-transient-20261003/summary.json`（SHA-256 `e111ff273f5904d0ad647f2512ca6967cfc8ea6eff144023dc62cf4f0a97d078`）：候选固定 setup 的长块 residue 仅编码每声道前 880/1024 个 MDCT bin；scratch 两字节扩至全频带后，两个控制的 SNR 明显改善，仍低于独立 libvorbis 参照。正式候选与 vendor 未改；后续已从固定提交 `e1b3a26` 独立构建并完成 19 输入的扩带回归，34 次编码与双路完整回读通过，4 次原 guard 拒绝符合预期；音乐整体改善和局部退步同时保留，下一步继续许可语料、听感、seek 与原生平台，不直接采用试探量化参数。该批构建引用了同期正在修改的 media，证据以记录的二进制、codec 源码和一致 PCM 为界，不声称整个工作区构建输入不可变；曲库及听感继续独立验收

## 后续里程碑

1. 10 · 有界四对象资源接收：本批软件实现与原始字节 / 故障检查通过，后续最大容量与双机验收
2. 10 · 网络会话生命周期：单局实时输入、单玩家水位、预约 Kira、伙伴反馈、取消清理与结束确认已有软件证据，正常完成后同进程下一局已交付，同 epoch 原音源软件续演已验证，继续长曲漂移、实际 Kira 丢包场景和网络故障，随后取得真实双机证据
3. 05 · 生产音频与曲库导入：完成编码数值域、瞬态、seek、长曲、听感和跨平台准入，手工源导入、Ready 曲库选择与游戏内配对源入口已接通，继续完整曲库、自动分析 / 内容接线
4. 06–08 · 音乐标注与内容编译：取得独立人工标注，改善 MIR，校准 Anchor 置信度并完成舞台自动编排与版本回放身份
5. 09 · 编辑器诊断闭环：只读 Replay、候选与拒绝证据、歌曲试听、明确版本的视觉 Replay 和显式软件计时关联已交付，继续独立音乐标注与完整内容接线，物理设备计时另验
6. 01–04 / 11–12 · 实际验收与发行：完成 UI / 场景真实性能、音频 loopback、双手柄和混合设备、平台显示、真人双人体验、网络故障与真实双机，最后干净机器四目标运行和实际 tag Release

资源接收 → 时钟与生产接线 → 网络验收可与内容路线并行；生产音频 → MIR / Anchor 策略 → 完整内容编译按实际依赖交付，编辑器证据视图可复用现有提案与 Replay 契约先行。完整 V1 最终汇合技术、设备、真人和发行门槛，软件实现与外部验收分别记录

## 分工与完成判断

主线程统一 Cargo、CI、台账、运行时接线、文档与里程碑提交；当前并行分工覆盖界面与输入契约、场景与反馈、针对性验证，互不覆盖持有文件。品牌线程保留品牌模块、WGSL 与资源的修改权，按实际交付版本补验

需求确认、代码完成、软件验证、真实设备和真人验收分别记录。本轮自动目标以软件功能、适当验证和里程碑提交全部完成为结束条件，随后等待用户真实验收；正式 V1 完成仍须原路线图及 02a 的全部退出证据，自动目标结束不等同于已通过真实验收，未运行条目保持 NOT RUN

当前软件里程碑已接通手工包播放、Anchor 提案与明确采用、v2 曲桥舞台以及保真编辑 / Replay JSONL 诊断。时间背景门控保留质量 FAIL；预装同包的两进程 QUIC 可靠历史会话已通过软件验证，已交付有界四对象原字节接收、原生游戏接线及正常完成后同进程下一局，故障后新轮次恢复已有软件证据，同 epoch 原音源软件续演已取得证据，长期漂移、容量与设备验收继续推进，内容路线仍需利用原创曲目来源清单取得独立音乐标注；原生波形、候选审阅、只读 Replay 与歌曲试听工作台已有软件和窗口证据，继续计时关联及原生观看；音乐标注、完整 MIR、舞台自动编排、编辑与联网仍按各自任务和证据交付

2026-10-07 · 后续软件交付：只读候选工作台已取得 28 项正式 lab 测试、Clippy / 格式 / 边界检查及三组窗口 11 PNG 证据；MIR small0 / BSD DBN 研究的源与 canonical 回读各 120 行数值通过，算法质量仍 FAIL，不接成已准入分析器。编码器四目标原生 CI 与下载证据核验已成功，seek、源导入、舞台版本与故障重入已交付，曲库和视觉观看继续按实际结果接线

2026-10-07 · 舞台身份与故障生命周期交付：Replay v2 从实际 StagePlan 记录版本，protocol v5 在握手、资源完成与真实 PCM 装载边界检查，历史 v1 保持 core-only；故障后已在同一进程使用新邀请 / epoch 开启新局。下一步完成生产源导入与工作台试听的已实现接线收尾，再推进完整视觉 Replay、曲库与内容分析能力，真实设备与真人验收保持独立

2026-10-07 · 生产源导入与工作台试听交付：准入 q10 编码器已被实际手工四对象导入消费并由生产 package loader 验证；三种工作台的试听控件、独立音频源游标与错误保留已实现，MockBackend、最终窄测及构造 UI 软件图通过。后续软件任务为实际视觉 Replay、UI / 场景测量、曲库入口及完整分析 / Anchor / 编排能力；真实输出设备、计时和真人验收单独保留

2026-10-07 · 原生只读视觉 Replay 已交付：`--watch-replay` 使用明确 Stage 1 / 2、原事实顺序与实际 Kira acknowledged cursor，完整开场后明确确认才播放；暂停、恢复、原 epoch 重启及返回菜单保持原包 / Replay，完整与 partial 不补造历史。133 项 runtime 测试、最终标题窄测、Clippy / 格式通过，四个实际窗口 case 与最终三张静态标题图分别绑定冻结二进制；证据和历史失败见[观看记录](../testdata/synthetic/visual-replay-observations-20261007.json)，历史 shader、设备计时及真人验收另计

2026-10-08 · 同 epoch 原音源软件恢复交付：27 项 net 测试、另 2 项真实 host loopback、160 项 runtime、3 项 sampler、Clippy / 格式与固定 game 构建通过；真实 UDP deadline 模型和实际 Kira 主动维护双进程分别取得软件证据，原 epoch / 音源 / 历史保持，恢复期负向 Hit 被过滤，双方终局各 3393 条事实 / 17 个 core 事件一致。旧 gate FAIL、QA 误断言与首次 sampler 命令清单 INCOMPLETE 均保留，最终完整 manifest 原生运行 PASS，见[恢复观察](../testdata/synthetic/same-epoch-recovery-observations-20261008.json)

当前自动目标仍在推进：下一批完成共享生产源导入入口、优化 release 的 UI / 场景性能矩阵，以及许可宽松原生分析与完整内容接线；MIR 研究未准入，不引入 GPL 或产品 Python。长期声卡漂移、真实双机、实体手柄 / 混合输入、DAC / 扬声器与真人验收继续保持 NOT RUN，软件里程碑通过不等于完整项目退出

2026-10-08 · 共享源导入与游戏 CLI 已验证：3 项 authored、30 项 lab、19 条 lab CLI 和 6 项 game 拒绝通过；445 项真实冻结输入与源码副本一致，新 `--import-authored` 从原创源实际创建包、完整开场后停 Ready 并等新确认，原 Kira 播放和 13 次双人软件输入完成，game / wrapper exit 0。观察见[导入记录](../testdata/synthetic/authored-game-import-observations-20261008.json)，完整自动 MIR 与 Ready 文件选择仍继续推进，下一批为新优化 release 的性能矩阵；恢复提交 `2a5fe59` 的 [Lightweight CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37655665012)已通过，设备 / 声学 / 真人验收仍 NOT RUN

2026-10-08 · 优化 release 原生性能观察：445 项冻结输入一致，实际 Linux / CPAL / AMD Vulkan 原环境矩阵 13 VALID、1 Wayland VSync 超时；最慢有效 limited60 两次复测 VALID，另一个隔离 X11 VSync 补测 VALID，未覆盖原失败。无限制组 Running p95 为 1.820–3.585ms，limited60 三次为 16.732 / 16.734 / 16.731ms，X11 补测 p95 / p99 为 33.320 / 33.673ms；记录的是 main-update 间隔与含探针缓存的 RSS，不是呈现 FPS、GPU 耗时或跨硬件预算。见[性能记录](../docs/runtime-performance.md)，共享源导入提交 `a617be3` 的 [Lightweight CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37658605442)已通过；继续 CID 换行、MIR 能力与完整内容接线，设备 / 声学 / 真人验收保持 NOT RUN

2026-10-08 · analysis v2 软件交付：schema / media / lab 共 80 项检查、runtime 内容 8 项、真实 v2 字节拒绝窄测、四包全目标 Clippy、格式和冻结 lab 构建通过。14 条真实 CLI 保留旧 v1 四对象与原 Replay 身份，新 final Ogg / 原 WAV 制作默认 v2，音频 / chart 字节不变，analysis / hash 按版本变化；两次旧 Replay 对新身份的拒绝符合预期，见[能力格式观察](../testdata/synthetic/analysis-v2-observations-20261008.json)。完整 MIR 算法仍未准入，继续 Ready 手工源导入、许可宽松原生分析与内容路线；CID 修复 `9018cc1` 的 [Lightweight CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37668513677)已通过

2026-10-08 · Ready 手工源导入已接通：正常无参开场后从曲库选择配对源与 authoring，明确确认才在后台制作四对象并完整加载，成功后停 Ready 等新 Start；164 项 runtime 测试、真实 worker 窄测、Clippy / 格式 / 架构边界、13 语言新文案字形检查通过。固定 debug 图的七项源导入和六项原曲库回归、26 个实际 PID / 进程组退出通过，22 / 42 张截图仅五张源导入图有指定范围目检；未完成的忙操作子项和设备 / 声学 / 真人验收保留 NOT RUN，见[Ready 导入观察](../testdata/synthetic/ready-source-import-observations-20261008.json)。下一步继续许可宽松原生分析、独立标注与完整 MIR / Anchor / 内容编排

2026-10-08 · 长曲与软件计时：`9207073` 已将 Hit / 共振范围查询改为 std 有序索引，原 9 项 core 检查及逐步差分一致；[Lightweight CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37685653393)已通过，开发测试用时不作游戏帧率结论。后续显式本机 timing sidecar 保留原 Replay / CSV、当次 mapping anchor 和独立 source / callback 历史，core 9 / replay 11 / runtime 174 / lab 32 / xtask 4 检查、Clippy / 格式 / 边界、13 语言新字形通过；447 项冻结输入、32 条 CLI、两组原生游戏及一个成功的工作台详情窗口取得软件证据，首次 GUI QA 焦点失败和三图有限目检范围保留，见[计时记录](../testdata/synthetic/timing-sidecar-observations-20261008.json)。继续许可宽松原生 beat 候选、独立人工标签入口和完整内容编排，实验质量 FAIL 不升级为准入，物理设备 / 声学 / 真人与其他三目标 native 计时仍 NOT RUN

2026-10-08 · 独立人工标签软件交付：`label-source`、`import-labels` 和 `compare-labels` 已接正式 lab，40 项 Lab 测试、近上限 1 项重复窄测、最终 Clippy 与 449 项冻结输入构建通过，固定 64 秒包的 33 项真实 CLI 控制通过。来源四对象、原标签及已有输出保持，448 条报告左右原记录完整保留；首次测试断言 Clippy lint 和复查证据均保留，见[独立标签](../docs/independent-labels.md)。有谱面上下文的试听加外部 JSON 是当前人工记录路径，GUI 标签编辑 / 隐藏提示盲标未实现，实际真人标签、听感 / 可玩性、confidence 校准、跨平台 CLI 和加载至写出间完整来源替换竞态仍 NOT_RUN

2026-10-08 · 原生 beat 候选软件交付：Lab 显式实验入口复用最终 canonical 音频与手工四对象事务，固定 CPU SDK / 模型，候选 confidence=None，不自动 Anchor；media 46 / xtask 5、native 3 项复跑、最终 Clippy 和 463 项冻结构建通过，实际 12 条成功命令、四组同 spect 数值对照与 13 项事务 / 资源控制通过，手工导入四对象字节保持，初 op_ref lint 失败保留，见[原生候选](../docs/native-beat-candidate.md)。原 wholeSpect 19 PASS / 9 FAIL（整体 FAIL）与音乐质量 FAIL 不升级为准入；四目标 workflow 已接资源和解包 smoke，本次四目标 native run / 新 tag Release、十分钟成本 / 取消及设备 / 真人仍 NOT_RUN，当前目标继续 active

2026-10-08 · 独立 Labels 编辑软件：`workbench-labels` 已接正式 Lab，隐藏作者 / 候选 / section 提示，共用 final stereo PCM 波形试听；人工点 / 半开区间、三态判断、原生 EditableText / IME 软件门控、Apply / Cancel dirty 草稿及新侧车保存 / worker join 已实现。正式 57 项 Lab 测试、Clippy、465 项冻结构建和独立静态审阅通过，13 locale × 39 条文案的 Noto 覆盖第二轮通过，首轮 ≤ 字形失败保留；见[独立标签](../docs/independent-labels.md#原生-labels-工作台)。旧 CLI 实绩保持，原生 GPU / 窗口补验本候选编写时 NOT_RUN，OS IME / 物理输入 / 母语 / 真人盲标及 confidence 校准另验，该 UI 批次当时标签采用未接线，当前目标继续 active

2026-10-08 · Labels 原生补验收尾：首轮 4 项软件 case / 14 PNG 与小窗口视觉 FAIL 保留，布局修复后 5 项 / 17 PNG、11 张指定目检；法语编辑路径补验及最终清理文案后的法语窄测各 1 项 / 4 PNG 全目检，总 11 项 case / 39 PNG 按冻结版分别记录。最终 Lab 57 项、Clippy / 格式、465 项冻结输入及 13 × 31 条文案字形通过，已删除每语言 8 条未用文案，旧 39 条覆盖实绩保留；详见[Labels 验证](../docs/independent-labels.md#原生-labels-工作台)。最终仅法语小窗口补验，OS IME、物理输入、声学试听 / 计时、母语 / 真人盲标及校准仍 NOT_RUN，该 UI 批次当时标签采用未接线，当前目标 active

## 当前发行软件补验

固定 `0930d17` 的 CI 与 Windows x64 / Windows ARM64 / Linux x64 优化包、解包 Lab 十条实际软件命令均通过，清洁包实际排除字体 QA 脚本；Linux ARM64 Build 失败，annotations 记录 hosted runner 失联，最终日志不可用，Cargo exit / 最后 crate 为 `BLOCKED_LOG_AVAILABILITY`，SDK / 包 / smoke 为 NOT RUN，见 [清洁包矩阵](../testdata/synthetic/native-beat-release-observations-20261008.json)

[原始 `046a05b` 矩阵](../testdata/synthetic/native-beat-release-original-observations-20261008.json) 的三包 QA 排除 FAIL 与 ARM exit 143 保留，原因未知；下一次仅 ARM64 以 `--jobs 1` / GNU time 收集实际构建诊断，release 优化参数保持，正常 runner 才能留下完整记录，静态软件检查与六项 shell stub 窄控已通过，后续固定 ref 运行单独记录，本冻结矩阵时 GNU time 诊断构建与 RSS 为 NOT RUN，不据前三目标或失联消息宣称四平台完成或 OOM 修复

这些结果绑定 `0930d17`，不覆盖后续 Labels GUI / 采用提交，完整四目标软件、实际 tag Release 和设备 / 真人门槛继续推进，既有标注采用软件证据保持

2026-10-08 · Linux ARM64 独立发行软件补验：固定 `d3b2648` 的 CI 通过后仅运行 `37708701226`，优化 Game / Lab、下载包和十项解包 Lab 命令通过；GNU time 实际 wall 29:35.45、最大 RSS 11386900 kbytes、command / tee exit 0，见 [ARM 观察](../testdata/synthetic/native-beat-arm-release-observations-20261008.json)，旧 143 与失联原因保持未知

本轮仅 ARM64 覆盖 `d3b2648`，另三目标实绩仍是 `0930d17`，未宣称当前同 ref 完整四目标；候选左右 111 / 114 beats 的原值与历史差异保留，完整质量 / reference logits/q、取消与长曲成本、设备 / 真人及实际 tag Release 继续独立推进，目标保持 active

2026-10-08 · 同提交四目标发行软件收尾：固定 `8cdda9d` 通过 CI 后，Windows / Linux × x86-64 / ARM64 四个原生优化 Game / Lab 包及解包软件检查通过，48 条实际 Lab 命令为 44 条成功和 4 条预期缺失模型拒绝，八份左右声道 tempo 报告共 192000 行完成核验；包内源码、资源、许可台账与字体 QA 排除按该提交身份匹配，见[同提交四目标观察](../testdata/synthetic/release-four-target-8cdda9d-observations-20261008.json)及[原始证据索引](../testdata/synthetic/release-four-target-8cdda9d-observations-20261008-raw-index.json)

原 `07830f8` 四目标 FAIL 单独保留：Windows 两架构因未声明的 `random()` 构建失败，Linux 两架构完成构建后在首次实验导入的 Ctrl+C 注册处失败，tempo 和后续命令未运行，见[原 078 四目标失败观察](../testdata/synthetic/release-four-target-07830f8-observations-20261008.json)及[原始证据索引](../testdata/synthetic/release-four-target-07830f8-observations-20261008-raw-index.json)；更早 `0930d17` 三目标与 `d3b2648` 单 ARM 的历史矩阵保持各自源码范围

Linux ARM64 本次 `--jobs 1` 构建的 GNU time wall 为 13:03.42、最大 RSS 11453596 kbytes，属于单次 runner 环境观察，不作为预算或旧失败根因。此矩阵不验收 Game GUI、干净机器 / Windows VC 前置、实体输入 / 音频、真人音乐参考或实际 tag Release；旧 wholeSpect 19 PASS / 9 FAIL（整体 FAIL）与音乐质量 FAIL 保留，tempo 的 beat_unit / meter / confidence 仍为 None、质量 UNSCORED、production_admission=false；后续内容 / GUI 软件与设备 / 真人任务继续按各自证据推进，完整 V1 退出和实际 tag Release 未关闭

2026-10-08 · 显式自动分析软件：真实 HFC onset / 相邻 beat 区间 tempo 候选与结构 sections / cues 新包已接通，作者 Anchor 和最终音频身份保持；生产 media83 / Lab85、Clippy / 格式 / 边界 / Lab 构建通过，14组 / 16 WAV 的原 PCM32与编码回读32项预声明机制控制通过。实际 SDK 新包46 onset / 19 tempo，原 chart 1 Anchor / 0 cue 保持；提案46 UnknownConfidence / 0 Anchor，严格 reader 的空 MusicTruth 为 NO_COMPARABLE_COVERAGE，不评价音乐质量，见[软件观察](../testdata/synthetic/automatic-analysis-observations-20261008.json)。最初环境 flag / 发行布局拒绝保留，旧 wholeSpect / 音乐 FAIL保持，当前目标 active；新 Game / UI、600秒成本、四目标原生及真人准入继续后续验收

2026-10-08 · 自动结构候选原生接线：两首真实候选新包完成原生开场→Ready等待→菜单主控接管→新确认→Kira推进→原cue切换→EOF / 关闭；runtime174、Clippy / Game构建及两个窗口软件检查通过，9张实际地面Mesh与6张PNG分别验收，见[原生观察](../testdata/synthetic/automatic-structure-native-observations-20261008.json)。首轮两次85秒QA FAIL保留，生产输入与候选算法未改；当前目标active，完整MIR / 音乐编排、repetition、600秒成本、当前来源四目标及设备 / 真人退出继续后续推进

2026-10-08 · 重复关系软件接线：`compile-repetition-candidate` 已实现完整bin的top4种子lag / 连续对角区间候选，固定原坐标、原速原调、Candidate / Algorithm / confidence=None，原音频与chart / sections / cues / Anchor保持；生产media85 / Lab85、Clippy / 格式 / 边界及Lab构建通过。原PCM34 / canonical34项预登记机制控制已PASS，各13正向 / 21预期拒绝、TP17FP0FN0，canonical另10项软件控制通过；600秒analysis成功310.879648秒 / kernel1593272 KiB、repetition18关系成功97.871086秒 / 72360 KiB及各独立后验通过，结构原64边界capErr62.510735秒 / 65820 KiB零发布，完整性能准入另验，见[候选契约](../docs/native-structure-features.md#显式编译重复关系候选包)；当前目标active，旧wholeSpect / 音乐FAIL、完整MIR、语义舞台编排、Anchor校准及真人验收保持未完成

2026-10-08 · 候选消费者里程碑：重复关系双侧选择 / 试听与自动HFC onset MusicTruth对照已接生产，Lab92项、Clippy / 格式 / 边界 / 构建、两条生产回读、4条实际对照CLI、两原生窗及6PNG指定目检通过；46条onset原值、旧v1报告完整字节与600秒七资源reader保持，源码373和冻结输入 / 二进制守恒，见[消费者观察](../testdata/synthetic/candidate-consumers-observations-20261008.json)。首窗鼠标QA期望FAIL保留，仅改QA一行后重验；当前目标active，下一批推进标注驱动置信校准 / 策略、600秒结构成功出版、运行期双源漂移维护及最终四目标加固，旧音乐质量FAIL与设备 / 真人退出保持

2026-10-08 · 600秒结构成功出版软件补验：`c63cf15`固定源码与Lab完成原PCM→q10 canonical→候选完整出版→typed守恒→Stage计划 / 起点消费8步，唯一边界14401536误差0，compile91.763316秒 / kernel RSS65916 KiB，原Source与新四对象保护、进程全部退出，见[长曲观察](../testdata/synthetic/structure-long-publication-observations-20261008.json)。原75转场capErr与音乐质量FAIL保持；当前目标active，继续校准 / 策略、双源漂移维护及最终四目标软件加固，真实设备 / 真人另验
