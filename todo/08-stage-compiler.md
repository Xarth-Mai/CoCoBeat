# 08 · StageCompiler

前置：07 的内容结构稳定；用户已批准以实际手工 SongPackage 契约并行推进软件基础，完整自动编译与体验退出仍依赖 07

- [x] 用直道、缓弯、广场、霓虹拱门、桥、段落门和终点组合 StagePlan
- [x] 轨道采样由 SongTime 确定，检查位置连续与预告可见性
- [x] 编译确定性与版本身份可回放，不以物理碰撞决定音乐判定
- [x] 装饰表现与关键几何分开；Resonance 不改变已显示的行进问题
- [x] 完整分析中的重复关系与实测能量驱动独立环境编排，保留 Unknown 和旧 Stage 1 / 2 几何

退出条件：同样内容/配置的舞台一致，Anchor 预期稳定，自动生成不破坏已验证的手写体验

## 已实现的软件基础

`cocobeat-stage` 只依赖 schema 和标准库，当前编译版本 3 复用版本 2 的真实 analysis 区间几何，生成直道、广场、缓弯和低桥：短于 16 秒为广场，长段前半缓弯、后半低桥，空隙保持直道，标签和置信度不参与几何选择。整数 SongTime 决定距离、侧移、抬升和切线；位置使用四次曲线，解析中心线端点位置与导数回零，整数采样和 Plaza 宽度的精度边界见 [SongPackage](../docs/song-package.md)

当前完整分析入口 `compile_analysis` 保留原重复关系和实测能量证据；已声明重复关系按确定性连通分组安排一致配色，已知空关系为 neutral，缺失能力为 Unknown，重叠冲突保守归为同组。Measured 且 Candidate / Validated 的能量只按双声道最大 RMS 的 1/32、1/8 阈值分档，原 RMS / peak 位保留；runtime 复用建筑与拱门材质，装饰不改变 Anchor、SongTime、几何与核心判定

runtime 共享一次编译的 Arc，以固定九张动态网格、两个装饰拱门和真实终点表现舞台；地面、Anchor 与段落提示共享三轴相对位置，画质和 Resonance 不修改计划。建筑避让最大弯道，桥坡反馈贴合切线并保留净空；Precise / Good 仍使用相同运动时长和不同强度

`cocobeat-lab inspect-stage PACKAGE FRAME` 可验证包并检查整数采样；开发歌曲保留既有手写场景。257 个横断面优先保留歌曲首尾与长特征接缝，密集短 Plaza 才使用基础条带近似，完整计划保留；舞台身份为完整内容身份加 compiler version，Replay v1 缺版本保持未知，v2 保存实际 StagePlan 版本；`inspect-replay-stage` 按明确 Stage 1 / 2 / 3 重建，旧 1 / 2 保持原几何，3 另绑定上述独立装饰规则，runtime 的 `--replay` 仍只校验 core，`--watch-replay` 已接同一版本几何与原事实观看；历史 shader 没有记录

v2 的 121 项相关测试和独立有理数参考的 60,480 字段比对通过，Clippy、格式、依赖边界与 game/lab 构建通过；42 项真实包 CPU 用例及最终 16 张静态 GPU 图通过，原拱门遮挡的失败图、修复与定向补验均保留，见 [验证策略](../docs/testing.md#缓弯低桥与同一轨道上的预告)。完整自动编排、设备性能与真人预告可读性继续保留，软件实现不替代阶段退出

2026-10-07 · 明确几何版本接线：支持按版本 1 / 2 确定性编译，Session 记录实际 StagePlan getter，Replay v1 缺失版本保持未知、v2 严格记录；网络身份在握手、资源完成与实际解码前后核对，未知版本不 Ready。184 项相关测试、Clippy、12 个真实 wire 拒绝及 3 个实际完整 PCM 控制通过，见[持久观察](../testdata/synthetic/stage-version-observations-20261007.json)。`inspect-replay-stage` 为整数几何重建，此处记录当时整数几何证据，后续原生观看见下文

2026-10-07 · 原生只读视觉 Replay 已交付：`--watch-replay` 使用明确 Stage 1 / 2、原事实顺序与实际 Kira acknowledged cursor，完整开场后明确确认才播放；暂停、恢复、原 epoch 重启及返回菜单保持原包 / Replay，完整与 partial 不补造历史。133 项 runtime 测试、最终标题窄测、Clippy / 格式通过，四个实际窗口 case 与最终三张静态标题图分别绑定冻结二进制；证据和历史失败见[观看记录](../testdata/synthetic/visual-replay-observations-20261007.json)，历史 shader、设备计时及真人验收另计

2026-10-08 · 独立标签的明确 Anchor 采用复用既有四对象导出与 Stage v2 加载，analysis.sections 保留原字节，几何规则和版本不变；真正改谱生成新 CID，Stage / Replay 继续绑定该身份，旧录制不自动迁移。本批 Stage / Replay 接线的软件对照通过，见[采用验证](../docs/independent-labels.md#明确采用为-anchor)，自动音乐结构、设备性能与真人体验不由手工采用路径准入

2026-10-08 · 完整计划来源诊断：`inspect-stage-plan PACKAGE` 已复用同一 Stage v2 输出全部片段和原 section 索引，旧能力缺失保持 Unknown、人工来源保持 Authored / NotRun / None，原 beat 候选不补造自动段落。66 项 Lab 测试、Clippy / 格式、466 项冻结输入构建与五包 30 条真实 CLI 通过，四对象保持，见[完整计划说明](../docs/song-package.md#内存-stageplan-与整数采样)、[持久观察](../testdata/synthetic/stage-plan-observations-20261008.json)与[原始证据索引](../testdata/synthetic/stage-plan-raw-index-20261008.json)。本批补齐软件诊断，完整自动音乐结构 / 编排、设备性能与真人体验退出继续保留

2026-10-08 · 自动结构候选游戏接线：真实 `compile-structure-candidate` 新包以原SectionCue帧完成两个原生窗口，短曲Plaza与长段Plaza / Curve / Bridge实际经过，9张地面Mesh只读观察和6张PNG分别通过；完整开场、Ready无自启、菜单接管后的新确认、实际Kira cursor和EOF / 关闭通过，见[软件观察](../testdata/synthetic/automatic-structure-native-observations-20261008.json)。首次QA漏独立主控接管导致两次85秒FAIL保留，生产输入 / Stage规则不改；runtime174、Clippy、Game构建通过，音乐编排质量、长曲性能、四目标与真人体验退出继续保留

2026-10-09 · Stage 3 上下文编排软件检查通过：295 项初测、等价版本范围修正后的 Replay11 / Clippy / 格式 / 边界和当前 Game / Lab 构建，七包 98 条实际命令核对 33 帧几何保真与原重复 / 能量证据，四张指定静态 GPU 图完成范围目检，见[观察记录](../testdata/synthetic/stage-context-observations-20261009.json)。首次 lint 失败保留，音乐质量、连续体验、实际联网身份、设备性能与四目标发行继续验收
