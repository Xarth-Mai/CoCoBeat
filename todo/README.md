# 路线图

Day 0 只创建当前有独立责任的 workspace，不预建最终目录树。复选框表示已有证据，不表示打算做。

当前优先级与里程碑见 [当前执行目标](current-goal.md)，分工、依赖和实际验证状态见 [工作进度](progress.md)

UI 重设计已进入 [02b · 界面与 HUD](02b-runtime-ui.md)，与 [04 · 雨夜霓虹](04-rain-neon-art.md) 的场景和反馈并行推进

| 阶段 | 目标与退出条件 |
|---|---|
| [00 工程底座](00-bootstrap.md) | workspace、SongTime、边界与本地检查成立 |
| [01 计时实验](01-timing-lab.md) | 输入/输出时间有测量证据，ClockBridge 的误差明确 |
| [02 本地垂直切片](02-local-vertical-slice.md) | 64 秒同机双人，Hit → Sync → 反馈 → Replay 闭环 |
| [02a 设置与显示](02a-runtime-settings.md) | 设置原子保存、13 个语言变体界面、画质和窗口显示分别验收；软件实现可与设备准备并行 |
| [02b 界面与 HUD](02b-runtime-ui.md) | 菜单、双人状态与舞台形成清楚的信息层级，全部现有操作及多语言布局可达 |
| [03 身体优先测试](03-body-first-playtest.md) | 观察到倾听、模仿与共同动作，记录未成立的假设 |
| [04 雨夜霓虹](04-rain-neon-art.md) | 表现强化共同动作且不破坏可读性 |
| [05 标准音频](05-canonical-audio.md) | 创建 media，唯一编码路径通过回读与互操作门槛 |
| [06 MIR 基准](06-mir-benchmark.md) | 原始 PCM / 编码回读、合成 / 人工数据形成对照 |
| [07 Anchor 编译](07-anchor-compiler.md) | 精度优先，低置信度留空，输出可复现 |
| [08 舞台编译](08-stage-compiler.md) | 创建 stage，生成连续、可预期的时间轨道 |
| [09 编辑与重放](09-editor-replay.md) | 创建 editor，内容修改与重放可检查、可回溯 |
| [10 QUIC](10-quic-network.md) | 创建 net，两端使用相同内容与权威事实历史 |
| [11 两台机器](11-two-pc-validation.md) | 真实设备与模拟网络下规则一致、延迟可测 |
| [12 V1 加固](12-v1-hardening.md) | 干净机器发行、许可与技术门槛通过 |

第一周争取完成 00 → 01 → 02，按证据推进，不把日历当验收。若双人核心体验不成立，返回 02/03 调整，不能用后续内容管线掩盖问题。
