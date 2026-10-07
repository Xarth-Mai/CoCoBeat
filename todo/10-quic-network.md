# 10 · QUIC 会话

前置：09 的身份和重放稳定。此时创建 cocobeat-net；core 不认识 Quinn。

- [x] 最新稳定 Quinn、会话证书指纹邀请、协议版本握手与内容哈希协商。
- [x] 有界可靠资源流，校验后才能 Ready；不在客户端重新生成“应该一样”的内容。
- [x] datagram 仅用于可丢时钟探测；控制、输入历史、水位与资源使用可靠 stream。
- [ ] 明确 ClockSync、ScheduleStart、epoch、重启/断线和 Finish/FinishAck。
- [x] 每位玩家可靠输入流与进度水位：水位关闭前不能把未到达当作未按键。
- [x] 权威 DuoEngine 与 Replay 走同一规则入口；共享确认可延迟，本地 Hit 不等待。

退出条件：相同完整历史得出相同结果，资源不一致不能开始；明确仅支持可直达端点，不暗示 NAT 穿透/中继能力。

2026-10-03：预装同包的 headless 会话已由 lab 的 `net-host` / `net-join` 消费，7 项 net 与 4 项边界测试、Clippy、构建和 13 项真实 loopback 场景通过；30 条进程命令覆盖真实 TLS、能力与内容身份、单条/64条批次、断线前缀和应用 Ack，135 条完整事实得到相同 16 个事件。断线时真实前缀保留且不补 Miss，错误/缺失 FinishAck 不报告 COMPLETE；入口与边界见 [网络会话](../docs/network-sessions.md)

可靠历史、四对象资源接收、进程单调 ClockSync / 未来 ScheduleStart 和单局生产输入 / 音频 / 伙伴表现已完成软件闭环，生产重开 / 重入继续开发；实际双机、LAN/WAN、防火墙、物理设备和最大容量性能为 NOT RUN，不能用 loopback 退出完整阶段

2026-10-07：新增 `net-receive`，protocol / ALPN v2，以固定顺序和原始字节接收四对象，完整媒体校验与包身份成功后经 Installed / InstalledAck 进入 Ready；25 项定向单元测试、16 条实际 loopback 命令及 6 类恶意资源传输通过，失败无 Ready、坏包无发布且 staging 清理，证据见 [验证策略](../docs/testing.md#quic-四对象资源接收)

2026-10-07 后续批次：protocol / ALPN v3 已实际消费 ClockSync、有限 datagram 探测、可靠 ClockSynced、未来 2 秒 ScheduleStart / Ack 和软件预约等待；13 项 net 测试、16 条实际命令、5 组时钟故障及 6 类资源回归通过。游戏窗口的实时输入、单玩家水位、音频预约与伙伴表现仍未接线，重启 / 断线 / 结束需在该生产路径继续验证，故完整 ClockSync / 生产条目保持未勾选，见 [时钟验证](../docs/testing.md#quic-网络时钟与预约软件起点)

2026-10-07 实时接线批次：protocol / ALPN v4 的有界 worker 已接原生游戏，完整包解码后显式 Ready，预约 Kira 播放并经双方 Armed 门控；各端仅关闭本玩家水位，本地 Hit 即时反馈，共享反馈等待可靠历史确认，正常结束完成 FinishAck，失败保存真实前缀并在关闭窗口前等待 worker 清理。134 项定向测试、7 组实际 live loopback、两个原生游戏进程和 16 条 headless 回归命令通过；两端实际 71 条事实与 5 个事件一致。生产重开 / 重入尚未实现，真实双机、物理输入 / 音频与容量性能保持 NOT RUN，见 [实时接线验证](../docs/testing.md#quic-实时游戏与原生软件接线)
