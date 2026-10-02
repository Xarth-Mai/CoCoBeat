# 10 · QUIC 会话

前置：09 的身份和重放稳定。此时创建 cocobeat-net；core 不认识 Quinn。

- [ ] 最新稳定 Quinn、会话证书指纹邀请、协议版本握手与内容哈希协商。
- [ ] 有界可靠资源流，校验后才能 Ready；不在客户端重新生成“应该一样”的内容。
- [ ] datagram 仅用于可丢时钟探测；控制、输入历史、水位与资源使用可靠 stream。
- [ ] 明确 ClockSync、ScheduleStart、epoch、重启/断线和 Finish/FinishAck。
- [ ] 每位玩家可靠输入流与进度水位：水位关闭前不能把未到达当作未按键。
- [ ] 权威 DuoEngine 与 Replay 走同一规则入口；共享确认可延迟，本地 Hit 不等待。

退出条件：相同完整历史得出相同结果，资源不一致不能开始；明确仅支持可直达端点，不暗示 NAT 穿透/中继能力。
