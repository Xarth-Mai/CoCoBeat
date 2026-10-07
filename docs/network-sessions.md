# QUIC 游戏与可靠历史会话

`cocobeat-net` 提供受邀请的双端会话，游戏通过有界 `LiveSession` worker 使用实时输入与实际 Kira 音频，lab 的 `host` / `join` / `join_receive` 继续验证加速历史；双方使用完整验证的同一个 [SongPackage](song-package.md)，客机可以预装，也可以接收主机的四个原始对象，运行同一 DuoEngine 并保存实际录制结果

本轮支持可直达端点的一次邀请、一局演奏；重连、重新开局和声卡漂移校正继续在 [10](../todo/10-quic-network.md) 推进，真实双机与网络环境验收见 [11](../todo/11-two-pc-validation.md)

## 实时游戏

```sh
cargo run --locked -p cocobeat-game -- --package PACKAGE --net-host 127.0.0.1:0 INVITE.json HOST_OUTPUT
cargo run --locked -p cocobeat-game -- --package PACKAGE --net-join INVITE.json GUEST_OUTPUT
# 客机没有内容包时
cargo run --locked -p cocobeat-game -- --net-receive INVITE.json NEW_PACKAGE GUEST_OUTPUT
```

每端仍完整播放品牌开场，在 Ready 菜单由用户确认开始后才启动 worker；主机写出新邀请并等待客机。网络 Prepared 要求四对象与完整包已验证，窗口在后台完整解码 PCM，重新核对包身份、长度和最终规则边界后才发送本端 Ready

主机固定 P1、客机固定 P2；每端的本地第一套键盘 / 手柄绑定控制自己的网络角色，默认均为 F，第二个本地演奏槽不能提交伙伴 Hit。菜单主控仍独立于角色编号，可以显式接管；等待与预约阶段保持释放屏障，音乐实际游标前进后才能演奏

Scheduled 在未来 deadline 之前交给窗口，Kira 用原生 `start_time(Duration)` 预约，成功入队后才发送 Armed；双方可靠 Armed / StartConfirmed 屏障必须仍留至少 100ms。Kira 延迟从音频回调消费 sound 开始，存在回调和排队偏差；歌曲 SongTime 继续来自实际 Kira 游标和 AudioClockBridge，网络 offset 不替换音频计时，也不证明扬声器同步

本地 Hit 立即反馈，并可靠发送已经接受的整数事实；每端只关闭自己玩家的历史，伙伴 Hit 与水位收到后才进入同一 core / Replay 入口，共享确认可以延迟。输入数量无需预声明，End 必须匹配实际接收计数及本玩家最终水位，随后沿用权威 Replay 校验和 FinishAck

窗口失焦、暂停请求、音频错误、断线、队列满或非法输入会终止当前联网局并保存真实前缀；不会仅暂停单端继续演奏。关闭窗口先取消 worker，再通过帧循环等待线程完成保存后退出。完成或失败后可以保存、调整设置和退出，本次邀请不能重开；新一局重新启动并使用新的邀请与输出路径

实时命令 / 事件队列各最多 256 项，入队与轮询不阻塞界面，满队列明确失败而不丢事实；每条线上 Facts 可以容纳 1–64 项，当前窗口按已经捕获的事实逐项提交。真实输入交换从预约起点起最多歌曲长度加 60 秒，Prepared 后本机 Ready 最多等待 120 秒

可在联网命令末尾加 `--live-observation NEW_DIR` 取得原生窗口、实际 Kira 游标、逐帧 CSV、输入计时诊断及 Running / 终态 PNG。此入口在完整品牌动画后注入明确标记的合成 Start / Hit，并自动关闭；它验证软件生产接线，不提供物理键盘、手柄、扬声器同步、双机或真人证据

## 加速历史运行

在两个终端运行，替换为已有的包与模板路径，邀请文件和输出目录必须是源包外的新路径，其父目录必须已存在

```sh
cargo run --locked -p cocobeat-lab -- net-host PACKAGE HOST_REPLAY.json 127.0.0.1:0 INVITE.json HOST_OUTPUT
cargo run --locked -p cocobeat-lab -- net-join PACKAGE GUEST_REPLAY.json INVITE.json GUEST_OUTPUT
# 客机未预装内容包时，接收至父目录已存在的新路径
cargo run --locked -p cocobeat-lab -- net-receive NEW_PACKAGE GUEST_REPLAY.json INVITE.json GUEST_OUTPUT
```

host 在写好邀请后输出 `INVITING`，包含实际端口、公开证书指纹与新 epoch；本机命令允许端口 0 自动分配，跨机器需选择可直达的本机单播地址及实际可用端口，当前没有 NAT 穿透或中继

模板必须使用现有 Replay v1、完整包身份和 `duo-watermark-v1`，先经原 core 校验；host 取 P1 子序列，join 取 P2 子序列，保留原 seq、整数 SongTime 与玩家内顺序，明确绑定到此次邀请的新 epoch。Hit 限定 `[0, canonical_frames)`，选中玩家最后一项必须是 `canonical_frames + confirmation_delay_frames + 1` 的显式水位；不在断线或 EOF 时补水位

双方完成时钟探测、Ready 和未来 ScheduleStart 屏障，等待各自预约的进程单调时间后加速发送历史，本入口不等待歌曲实际时长，也不把模板时间解释为网络抵达时间。双方预检并协商事实计数，合计最多 160,000 项；每玩家可靠有序流携带 Hit 和关闭此前历史的水位，主机负责权威结算

## 资源接收

`net-receive` 先完成标准 TLS、邀请能力和 Welcome 校验，再检查四对象长度 / 总量，按固定顺序接收原始字节并核对各自 BLAKE3、类型、epoch 和严格 EOF；媒体事务继续检查 manifest、对象引用、schema、内容语义、完整音频严格读回及预期包哈希，成功后才发布目录。客机使用已验证内容检查本地 Replay 模板，再发送 Installed；主机确认完整身份与事实容量并回复 InstalledAck，之后双方才进入 Ready

主机对已验证对象快照的四个哈希再次核对，发送中检查长度和哈希，防止源路径变化被当作相同内容；接收方保留原始编码字节，不重新编译 Analysis / Chart 或转码音频。包路径与会话输出需为两个独立的新路径，已有目录 / 文件 / 符号链接和解析后的别名均保留

资源阶段失败写入 `status.json`，尚未初始化规则会话时不生成 Replay；任何已经成功发布的有效包都会保留，例如后续 Replay 模板不匹配时 `package_received=true` 而会话为 FAILED。仅部分对象、坏哈希、截断、尾随数据或错误包身份不会发布；未经完整验证不会发送 Installed / Ready

## 时钟与未来起点

本端各自使用 Session 初始化时的进程单调 origin；ClockProbe / ClockReply 为固定 48 字节 datagram，含 epoch、probe id 和四时间戳。guest 最多发 8 次探测、每次等待 250ms，整个探测阶段限 5 秒、入站最多 64 个包；损坏、旧 epoch、错误 id、重复或不匹配的 reply 丢弃，无有效样本则失败，不能 Ready

guest 用可靠 ClockSynced 提交选中的探测，host 核对实际发送的回复记录和同一 ClockSync 模型；模型保留网络不对称的 offset / RTT 区间，样本老化和未来预约的漂移也扩大误差。默认相对漂移上界假设为 1000ppm，样本有效期 2 秒、单次交换最多 1 秒；这是需实测检验的进程时钟假设，不能当作设备输出、声学精度或实际双机同步证据

双方 Ready 后，host 可靠发送自己单调 origin 上未来 2 秒的 ScheduleStart；guest 必须使用新鲜样本把整个可能起点区间映射至本端，仍留至少 100ms 的准备时间，再可靠回复 ScheduleStartAck。host 重算并核对映射、epoch 和同一 deadline，Ack 未在准备期限前到达则失败；双方等待预约的 Instant，断连阻止开始，实际唤醒迟到超过 100ms 则记录并失败，不发送输入历史

`status.json.network_timing` 保留探测计数、样本、host / local deadline、映射 uncertainty、实际软件唤醒与迟到；未达到的步骤用 null，host 映射 uncertainty 为 0 只表示本端坐标恒等。加速历史入口的输入时间来自原 Replay；实时入口来自本端 AudioClockBridge。两个入口都保留整数 SongTime，也不保证通信崩溃或确认后分区时的分布式原子开始

## 身份与输出

每次 host 生成新自签证书、256 bit 随机邀请能力和随机 epoch，协议为 `cocobeat-session/4`，旧 v1 / v2 / v3 邀请拒绝。客户端先校验邀请内部的证书 BLAKE3，再使用标准 TLS 1.3 信任验证，并在发送能力 secret 前核对远端 leaf DER 完全相等；公开指纹不代替邀请能力，邀请应通过双方认可的渠道传递

邀请严格限制为 16 KiB，证书最多 4 KiB，不接受未知字段或版本；Unix 新文件权限为 0600，Windows 继承实际父目录 ACL。命令状态与 Replay 不输出 token 或私钥，每次 host 只接受一次连接尝试，失败后重新运行会生成新邀请

每端输出目录保留实际 `live.replay.json` 和 `status.json`；主机在两侧完整历史结束后生成 `authority.replay.json`，客户端核对长度、哈希、身份、epoch、完整逐玩家子序列和全部 core 结果，保存成功后才发送应用 `FinishAck`

主机验证 Ack 后正常关闭；客户端观察该关闭后才报告 `COMPLETE`。发生错误时保留已经成功 ingest 的真实前缀，状态为 `FAILED`，或已验证权威 Replay 的 `AUTHORITY_VERIFIED_UNCONFIRMED`。传输 EOF 和 transport ACK 不等同于应用 FinishAck；通信中断时两端可能观察到不同完成状态，协议不保证分布式原子提交

已有输出不覆盖，源包内及父目录符号链接别名拒绝；保存失败作为错误返回，不能把未写出的文件报告为已保存。成功后的权威 Replay 可以交给 [JSONL 诊断](replay-diagnostics.md) 检查实际 Hit、判定与水位

## 限额与边界

消息使用 4 字节大端长度和严格 JSON，单条最多 16 KiB，每批 1–64 个事实，单玩家输入累计最多 32 MiB；加速历史队列最多 4 批，实时命令 / 事件队列各最多 256 项，可靠流采用背压，队列压力不能静默丢输入。资源流采用固定 16 字节类型 / epoch 头和固定顺序的四个原始对象，不接受对端文件名；单对象上限依次为 512 MiB、16 MiB、4 MiB、64 KiB，总量最多 532 MiB + 64 KiB，传输缓冲为 64 KiB。权威 Replay 沿用现有 20 MiB 上限，收发窗口与固定 stream 数另有限制，datagram 只承载时钟探测，收发缓冲各 4 KiB

首次等待 guest 最多 120 秒，TLS / capability 阶段绝对限时 10 秒，资源传输异步等待最多 5 分钟，单次读写进展各 30 秒；ClockSync / Ready / ScheduleStart 屏障合计最多 30 秒、无输入进展各 30 秒，从预约起点开始的加速会话最多 15 分钟，最终 Replay 传输校验 60 秒、Ack 30 秒、端点关闭最多再等 5 秒。超时终止当前会话，不续用取消读取后的半条消息

这些限制约束协议字节、事实与异步等待，不能抢占同步文件访问、哈希、音频解码或 core 运算，也不代表达到最大事实量时仍有合理帧时；网络压力、平台设备和人体体验须按各自证据评估。包发布复用同文件系统 staging → 校验 → rename，已有目标包括符号链接拒绝，失败只清理当前调用创建的对象；本地其他进程在最终存在性检查后创建空目录的竞态仍沿用现有发布实现，rename 不提供平台专用的排他替换保证

net 仅依赖 schema / core / replay / media 与网络实现库，core 不认识 Quinn；runtime 依赖 net 的同步有界 worker 入口，Tokio 和 QUIC I/O 留在其拥有的线程
