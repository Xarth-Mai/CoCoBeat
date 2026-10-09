# 10 · QUIC 会话

前置：09 的身份和重放稳定。此时创建 cocobeat-net；core 不认识 Quinn

- [x] 最新稳定 Quinn、会话证书指纹邀请、协议版本握手与内容哈希协商
- [x] 有界可靠资源流，校验后才能 Ready；不在客户端重新生成“应该一样”的内容
- [x] datagram 仅用于可丢时钟探测；控制、输入历史、水位与资源使用可靠 stream
- [x] ClockSync、ScheduleStart、epoch、Finish/FinishAck 与正常完成后同进程新一局的软件闭环
- [x] Fault 后通过新邀请 / epoch 开启新轮次的软件恢复
- [x] 同 epoch 原进程、原音源与原历史的软件续演
- [x] Running 期间有界进程时钟维护与过期样本故障处理
- [x] 原始音源出版的有界相位检查与生产 Running 接线
- [x] 原 protocol8 声明整数软件源的真实 worker 双向三轮超 guard 校正与源身份拒绝，见[软件源观察](../testdata/synthetic/source-phase-model-observations-20261009.json)
- [x] protocol9 整数软件源的 active Phase 一次认证续接与原 round / deadline / floors / FIFO 保真，见[续接观察](../testdata/synthetic/source-phase-reconnect-observations-20261009.json)
- [ ] 实际 Kira 超 guard 校正、active Phase 认证续接与长期漂移故障矩阵
- [ ] 长期声卡漂移测量与校正
- [x] 每位玩家可靠输入流与进度水位：水位关闭前不能把未到达当作未按键
- [x] 权威 DuoEngine 与 Replay 走同一规则入口；共享确认可延迟，本地 Hit 不等待

退出条件：相同完整历史得出相同结果，资源不一致不能开始；明确仅支持可直达端点，不暗示 NAT 穿透/中继能力

2026-10-03：预装同包的 headless 会话已由 lab 的 `net-host` / `net-join` 消费，7 项 net 与 4 项边界测试、Clippy、构建和 13 项真实 loopback 场景通过；30 条进程命令覆盖真实 TLS、能力与内容身份、单条/64条批次、断线前缀和应用 Ack，135 条完整事实得到相同 16 个事件。断线时真实前缀保留且不补 Miss，错误/缺失 FinishAck 不报告 COMPLETE；入口与边界见 [网络会话](../docs/network-sessions.md)

可靠历史、四对象资源接收、进程单调 ClockSync / 未来 ScheduleStart、生产输入 / 音频 / 伙伴表现及正常完成后同进程新一局已完成软件闭环，Fault 后新轮次恢复已有软件证据，同 epoch 软件续演已有独立 UDP 模型与原生主动维护证据，长期漂移继续开发；实际双机、LAN/WAN、防火墙、物理设备和最大容量性能为 NOT RUN，软件通过不等同于真实验收

2026-10-07：新增 `net-receive`，protocol / ALPN v2，以固定顺序和原始字节接收四对象，完整媒体校验与包身份成功后经 Installed / InstalledAck 进入 Ready；25 项定向单元测试、16 条实际 loopback 命令及 6 类恶意资源传输通过，失败无 Ready、坏包无发布且 staging 清理，证据见 [验证策略](../docs/testing.md#quic-四对象资源接收)

2026-10-07 后续批次：protocol / ALPN v3 已实际消费 ClockSync、有限 datagram 探测、可靠 ClockSynced、未来 2 秒 ScheduleStart / Ack 和软件预约等待；13 项 net 测试、16 条实际命令、5 组时钟故障及 6 类资源回归通过。该批次尚未接入游戏窗口的实时输入、单玩家水位、音频预约与伙伴表现，生产路径随后由实时接线批次验证，见 [时钟验证](../docs/testing.md#quic-网络时钟与预约软件起点)

2026-10-07 实时接线批次：protocol / ALPN v4 的有界 worker 已接原生游戏，完整包解码后显式 Ready，预约 Kira 播放并经双方 Armed 门控；各端仅关闭本玩家水位，本地 Hit 即时反馈，共享反馈等待可靠历史确认，正常结束完成 FinishAck，失败保存真实前缀并在关闭窗口前等待 worker 清理。134 项定向测试、7 组实际 live loopback、两个原生游戏进程和 16 条 headless 回归命令通过；两端实际 71 条事实与 5 个事件一致。该批次验证单局，真实双机、物理输入 / 音频与容量性能保持 NOT RUN，见 [实时接线验证](../docs/testing.md#quic-实时游戏与原生软件接线)

2026-10-07 正常多局批次：联网命令可重复追加 `--next-round NEW_INVITE NEW_OUTPUT`，Finished 菜单在正常 COMPLETE、本地 Replay 保存成功和 worker 退出后消费一份后续配置；主机沿用包 / bind 并发布新邀请，客机等待新邀请，首局接收内容包后改用 Join 读取已发布包。整局状态重建清除音乐、反馈、Results、source clock、seq 与水位，新 Prepared 使用新 epoch；邀请与输出路径继续排他检查，旧局文件保留，最后一局及 Fault 无下一局入口

两个原生游戏进程已在同进程生命周期内连续完成两局，epoch 为 `3124893908200008438` / `1601151596960184129`，两局事实计数为 `[35, 36]` / `[36, 36]`，每局双方均有 5 个事件；新证书、seq 重置、Receive → Join、独立输出和旧局文件保真检查 PASS，见 [综合软件观测](../testdata/synthetic/session-diagnostics-observations-20261007.json)。本批为 loopback、实际 Kira source cursor 和合成输入的软件验证，真实键盘 / 手柄、扬声器同步、双机、LAN/WAN 与真人体验仍待验收

2026-10-07 · 协议 v5 纳入明确 Stage 编译身份，Replay v2 记录实际版本；旧 v1 原字节保持，12 个真实早期身份拒绝与 3 个实际 PCM 版本检查通过，见[Stage 观察](../testdata/synthetic/stage-version-observations-20261007.json)。故障后新邀请重入取得 127 项 runtime、9 组 worker 与 3 组原生双轮证据；旧前缀 / 文件保留，等待 worker 与 decoder 结束及录制保存，再明确消费新配置，见[网络观察](../testdata/synthetic/network-reentry-observations-20261007.json)。同 epoch 续演与真实设备 / 双机验收继续保留

2026-10-08 · 同 epoch 软件恢复：protocol / ALPN v6 保留原 epoch、原 PCM / Kira 句柄和完整事实前缀，仅完成一次有界续演；27 项 net 测试、另 2 项实际 host loopback、160 项 runtime、3 项 sampler 窄测、Clippy / 格式与固定 game 构建通过。真实 UDP 黑洞由可靠 deadline 触发恢复，source 为整数 48kHz 模型；独立的实际 Kira 双进程主动维护恢复保持 source generation / source_id `1 / 1`，恢复期负向 Hit 被过滤，终局双方各 3393 条事实 / 17 个 core 事件及权威 Replay 一致

原 50ms guard 失败、QA 开场 flag 误断言和首次 sampler 运行的命令清单 INCOMPLETE 保留，最终重新原生运行补齐完整 PID / 命令 / exit 证据，固定二进制和完整源码身份见[同 epoch 恢复观察](../testdata/synthetic/same-epoch-recovery-observations-20261008.json)及[验证策略](../docs/testing.md#同-epoch-原音源软件恢复)。同 epoch 软件项 PASS 不关闭长期声卡漂移、实际 Kira 丢包场景、最大容量、物理设备和双机 / 真人验收

2026-10-08 · Running 时钟维护软件：protocol / ALPN v7 将原初始 / 续演 CBCK 与周期 CBMC 分域，每秒最多一轮，250ms / 64包 / 每epoch1024轮有界；双方只由实际匹配交换更新 ClockSync，旧轮次、丢失和重复不能刷新，轮次跨认证续接保留。原可靠读和事实 FIFO、一次续演预算及新 epoch 状态重建保持；net31、真实 loopback3、runtime175、Clippy / 格式 / 边界和当前 Game / Lab 构建通过

正常与真实双向 UDP 黑洞恢复双方权威 Replay 一致；黑洞以实际样本年龄 2006536911 / 2006500101 ns 超过原2秒有效期触发，两端均无 RequestRecovery，见[维护观察](../testdata/synthetic/live-clock-maintenance-observations-20261008.json)。本批只更新进程时钟映射，长期双音源相位校正未完成；原29秒可靠 deadline 分支的历史证据保留，本次未复跑该分支，实际 Kira 丢包、物理设备、双机与真人继续独立验收

2026-10-09 · protocol / ALPN v8 的原音源相位检查已接生产，net42 / runtime179 普通测试、三个真实 QUIC 窄测、Clippy / 格式 / 边界和当前 Game / Lab 构建通过；Gamescope 原生双方各完成两轮实际 Kira 原出版检查，完整差值均[-514,514]帧，原句柄 / source floors / 可靠 FIFO 和终局846条事实、5个事件及权威 Replay 保持。原 Xvfb 无 DRI3 的呈现失败保留；未触发实际校正，多轮漂移、active Phase 续接重绑及设备 /双机继续，见[相位观察](../testdata/synthetic/source-phase-maintenance-observations-20261009.json)

2026-10-09 · 当前 protocol / ALPN v9：net52 / runtime185与六项本机检查、四个真实 QUIC 窄测函数 / 五类 typed read 控制和四个坏历史拒绝通过；最新六模型实际成功，88份 proof /450个 guards 重算保持，原失败链保留。另两项当前 Kira 普通检查与同源普通恢复自然完成，833 /837条事实和权威一致；Kira 强制漂移 / active阶段重绑、AwaitLive、DAC /双机继续未验，见[续接观察](../testdata/synthetic/source-phase-reconnect-observations-20261009.json)

2026-10-09 · [Phase写出窄补](../testdata/synthetic/source-phase-control-write-observations-20261009.json)接通既有typed错误，net52 / runtime186、六项必要检查与两个真实QUIC部分写入控制通过；整帧预算保守消费、原状态 /事件次序和已接受Fact /Replay保持，非法domain /预算仍终态。完整认证续接、其他写入边缘与实际Kira scheduled /AwaitLive继续独立验收

2026-10-09 · [实际Kira Sampling续接](../testdata/synthetic/source-phase-sampling-native-observations-20261009.json)完成round1原源 /同epoch重绑和重新验证，随后round2正常维护、自然EOF /权威820事实一致；四原proof的完整区间独立重算均[-514,514]帧，原容量 /期限 /预算保持。请求在成功Sampling handler之后、not_before之前，未验PCM子窗内中断；实际强制校正、Pausing /pending-resume /AwaitLive、长期漂移和设备 /双机继续后续验收

2026-10-09 · [自有 Guest 单次暂停观察](../testdata/synthetic/source-phase-kira-owned-stall-observations-20261009.json)保持严格 `FAIL_PREDECLARED_COMPLETE_INTERVAL_OUTSIDE_GUARD`，首轮完整区间 `[-3714,-2174]` 跨过 guard，未满足整区间越界预登记条件；75.254205 ms 是进程 STOP / CONT 请求间隔，不能作为物理漂移。实际普通校正窄路径为 `SUPPORTED_WITH_SCOPED_INFERENCE`，真实 Paused / Resuming、原冻结帧 / source / sequence、独立 verification 和下一 Check `[-386,642]`、自然 EOF / 836 facts / 5 events / authority 一致有原件；PhaseScheduled 没有直接事件日志，外部 ledger 的原 summary 单项封口差异保留，未给 aggregate PASS。380 项来源已冻结，该严格故障 case 与完整矩阵仍未闭合，不改阈值或盲试；当前软件交付继续，设备 / 音乐 / 真人准入另验

2026-10-09 · [原生 Guest AwaitLive 续接](../testdata/synthetic/source-phase-await-live-native-observations-20261009.json)通过注册式真实状态请求、同轮重绑 / fresh verification、下一普通检查及自然 EOF；net53 / runtime186与六项检查通过，六 proof 独立区间重算保持原 guard，双方820 facts / 5 events权威一致。旧立即请求 QA_NOT_HIT及75ms严格FAIL保留；Host同时状态、全故障 / 四平台优化包、设备 / 音乐 / 真人仍分开验收，本轮提交与CI收尾
