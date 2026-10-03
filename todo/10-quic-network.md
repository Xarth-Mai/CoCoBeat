# 10 · QUIC 会话

前置：09 的身份和重放稳定。此时创建 cocobeat-net；core 不认识 Quinn。

- [x] 最新稳定 Quinn、会话证书指纹邀请、协议版本握手与内容哈希协商。
- [ ] 有界可靠资源流，校验后才能 Ready；不在客户端重新生成“应该一样”的内容。
- [ ] datagram 仅用于可丢时钟探测；控制、输入历史、水位与资源使用可靠 stream。
- [ ] 明确 ClockSync、ScheduleStart、epoch、重启/断线和 Finish/FinishAck。
- [x] 每位玩家可靠输入流与进度水位：水位关闭前不能把未到达当作未按键。
- [ ] 权威 DuoEngine 与 Replay 走同一规则入口；共享确认可延迟，本地 Hit 不等待。

退出条件：相同完整历史得出相同结果，资源不一致不能开始；明确仅支持可直达端点，不暗示 NAT 穿透/中继能力。

2026-10-03：预装同包的 headless 会话已由 lab 的 `net-host` / `net-join` 消费，7 项 net 与 4 项边界测试、Clippy、构建和 13 项真实 loopback 场景通过；30 条进程命令覆盖真实 TLS、能力与内容身份、单条/64条批次、断线前缀和应用 Ack，135 条完整事实得到相同 16 个事件。断线时真实前缀保留且不补 Miss，错误/缺失 FinishAck 不报告 COMPLETE；入口与边界见 [网络会话](../docs/network-sessions.md)

当前只完成可靠历史软件闭环，资源接收、音频 ClockSync / 未来 ScheduleStart、游戏输入与表现生产接线继续开发；实际双机、LAN/WAN、防火墙、物理设备和最大容量性能为 NOT RUN，不能用 loopback 退出完整阶段
