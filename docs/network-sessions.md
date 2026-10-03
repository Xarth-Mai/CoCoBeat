# QUIC 可靠历史会话

`cocobeat-net` 提供受邀请的双进程软件会话，lab 消费其同步 `host` / `join` 入口；双方先安装并完整验证同一个 [SongPackage](song-package.md)，用已有 Replay 模板提供各自玩家历史，经真实 QUIC 连接运行唯一 DuoEngine 并保存实际录制结果

这一步验证可靠历史、身份和结算协议；游戏窗口尚未接入网络，真实音频时钟、未来 ScheduleStart、输入采集、资源传输和重连分别继续在 [10](../todo/10-quic-network.md) 推进，双机与网络环境验收见 [11](../todo/11-two-pc-validation.md)

## 运行

在两个终端运行，替换为已有的包与模板路径，邀请文件和输出目录必须是源包外的新路径，其父目录必须已存在

```sh
cargo run --locked -p cocobeat-lab -- net-host PACKAGE HOST_REPLAY.json 127.0.0.1:0 INVITE.json HOST_OUTPUT
cargo run --locked -p cocobeat-lab -- net-join PACKAGE GUEST_REPLAY.json INVITE.json GUEST_OUTPUT
```

host 在写好邀请后输出 `INVITING`，包含实际端口、公开证书指纹与新 epoch；本机命令允许端口 0 自动分配，跨机器需选择可直达的本机单播地址及实际可用端口，当前没有 NAT 穿透或中继

模板必须使用现有 Replay v1、完整包身份和 `duo-watermark-v1`，先经原 core 校验；host 取 P1 子序列，join 取 P2 子序列，保留原 seq、整数 SongTime 与玩家内顺序，明确绑定到此次邀请的新 epoch。Hit 限定 `[0, canonical_frames)`，选中玩家最后一项必须是 `canonical_frames + confirmation_delay_frames + 1` 的显式水位；不在断线或 EOF 时补水位

双方收到 Ready / Start 屏障后加速发送历史，本入口不等待歌曲实际时长，也不把模板时间解释为网络抵达时间。双方预检并协商事实计数，合计最多 160,000 项；每玩家可靠有序流携带 Hit 和关闭此前历史的水位，主机负责权威结算

## 身份与输出

每次 host 生成新自签证书、256 bit 随机邀请能力和随机 epoch，协议为 `cocobeat-session/1`。客户端先校验邀请内部的证书 BLAKE3，再使用标准 TLS 1.3 信任验证，并在发送能力 secret 前核对远端 leaf DER 完全相等；公开指纹不代替邀请能力，邀请应通过双方认可的渠道传递

邀请严格限制为 16 KiB，证书最多 4 KiB，不接受未知字段或版本；Unix 新文件权限为 0600，Windows 继承实际父目录 ACL。命令状态与 Replay 不输出 token 或私钥，每次 host 只接受一次连接尝试，失败后重新运行会生成新邀请

每端输出目录保留实际 `live.replay.json` 和 `status.json`；主机在两侧完整历史结束后生成 `authority.replay.json`，客户端核对长度、哈希、身份、epoch、完整逐玩家子序列和全部 core 结果，保存成功后才发送应用 `FinishAck`

主机验证 Ack 后正常关闭；客户端观察该关闭后才报告 `COMPLETE`。发生错误时保留已经成功 ingest 的真实前缀，状态为 `FAILED`，或已验证权威 Replay 的 `AUTHORITY_VERIFIED_UNCONFIRMED`。传输 EOF 和 transport ACK 不等同于应用 FinishAck；通信中断时两端可能观察到不同完成状态，协议不保证分布式原子提交

已有输出不覆盖，源包内及父目录符号链接别名拒绝；保存失败作为错误返回，不能把未写出的文件报告为已保存。成功后的权威 Replay 可以交给 [JSONL 诊断](replay-diagnostics.md) 检查实际 Hit、判定与水位

## 限额与边界

消息使用 4 字节大端长度和严格 JSON，单条最多 16 KiB，每批 1–64 个事实，单玩家输入累计最多 32 MiB；有界队列最多 4 批，可靠流采用背压，不丢输入。权威 Replay 沿用现有 20 MiB 上限，收发窗口与固定 stream 数另有限制，datagram 尚未启用

首次等待 guest 最多 120 秒，TLS / capability 阶段绝对限时 10 秒，Ready / Start 和无输入进展各 30 秒，从 Start 开始的加速会话最多 15 分钟，最终 Replay 传输校验 60 秒、Ack 30 秒、端点关闭最多再等 5 秒。超时终止当前会话，不续用取消读取后的半条消息

这些限制约束协议字节、事实与等待，不能抢占同步 core 运算，也不代表达到最大事实量时仍有合理帧时；网络压力、平台设备和人体体验须按各自证据评估。net 仅依赖 schema / core / replay / media 与网络实现库，core 不认识 Quinn，runtime 尚不依赖 net
