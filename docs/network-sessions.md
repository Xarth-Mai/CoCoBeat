# QUIC 游戏与可靠历史会话

`cocobeat-net` 提供受邀请的双端会话，游戏通过有界 `LiveSession` worker 使用实时输入与实际 Kira 音频，lab 的 `host` / `join` / `join_receive` 继续验证加速历史；双方使用完整验证的同一个 [SongPackage](song-package.md)，客机可以预装，也可以接收主机的四个原始对象，运行同一 DuoEngine 并保存实际录制结果

当前支持可直达端点，每份邀请只用于一局；正常完成或故障终止后可以在同一窗口进入预先声明的新一局，逐局使用新邀请和独立输出路径。实时局内支持一次有界的同 epoch 续演，长期声卡漂移校正继续在 [10](../todo/10-quic-network.md) 推进，真实双机与网络环境验收见 [11](../todo/11-two-pc-validation.md)

## 实时游戏

```sh
cargo run --locked -p cocobeat-game -- --package PACKAGE --net-host 127.0.0.1:0 INVITE.json HOST_OUTPUT
cargo run --locked -p cocobeat-game -- --package PACKAGE --net-join INVITE.json GUEST_OUTPUT
# 客机没有内容包时
cargo run --locked -p cocobeat-game -- --net-receive INVITE.json NEW_PACKAGE GUEST_OUTPUT
# 正常完成或故障终止后在同一窗口开启新局，可重复追加 --next-round
cargo run --locked -p cocobeat-game -- --package PACKAGE --net-host 127.0.0.1:0 INVITE.json HOST_OUTPUT --next-round NEXT_INVITE.json NEXT_HOST_OUTPUT
cargo run --locked -p cocobeat-game -- --package PACKAGE --net-join INVITE.json GUEST_OUTPUT --next-round NEXT_INVITE.json NEXT_GUEST_OUTPUT
```

每端仍完整播放品牌开场，在 Ready 菜单由用户确认开始后才启动 worker；主机写出新邀请并等待客机。网络 Prepared 要求四对象与完整包已验证，窗口在后台完整解码 PCM，重新核对包身份、长度和最终规则边界后才发送本端 Ready

主机固定 P1、客机固定 P2；每端的本地第一套键盘 / 手柄绑定控制自己的网络角色，默认均为 F，第二个本地演奏槽不能提交伙伴 Hit。菜单主控仍独立于角色编号，可以显式接管；等待与预约阶段保持释放屏障，音乐实际游标前进后才能演奏

Scheduled 在未来 deadline 之前交给窗口，Kira 用原生 `start_time(Duration)` 预约，成功入队后才发送 Armed；双方可靠 Armed / StartConfirmed 屏障必须仍留至少 100ms。Kira 延迟从音频回调消费 sound 开始，存在回调和排队偏差；歌曲 SongTime 继续来自实际 Kira 游标和 AudioClockBridge，网络 offset 不替换音频计时，也不证明扬声器同步

本地 Hit 立即反馈，并可靠发送已经接受的整数事实；每端只关闭自己玩家的历史，伙伴 Hit 与水位收到后才进入同一 core / Replay 入口，共享确认可以延迟。输入数量无需预声明，End 必须匹配实际接收计数及本玩家最终水位，随后沿用权威 Replay 校验和 FinishAck

窗口失焦、暂停请求、音频错误、队列满或非法输入会终止当前联网局并保存真实前缀；可恢复的传输中断进入下述同 epoch 续演，超过期限或恢复失败后也终止当前局。关闭窗口先取消 worker，再通过帧循环等待网络及拥有的 PCM 解码线程结束后退出。完成或失败后可以保存、调整设置和退出；Ready、Finished 或 Fault 在线程结束及新配置可用后提供下一局，确认时先成功保存旧录制再消费新配置

每对 `--next-round NEW_INVITE NEW_OUTPUT` 声明一份后续配置；主机沿用原包与 bind，客机沿用原包，首局使用 `--net-receive` 的客机在确认新局时检查目标：尚不存在则继续 Receive，已发布的真实目录则使用 Join 完整复核，文件、符号链接及其他错误拒绝覆盖。Ready、Finished 或 Fault 菜单在当前网络及 PCM 解码线程已结束且仍有后续配置时提供下一局，确认时先成功保存本地 Replay 再消费新配置；客机还需等新邀请路径出现，缺失时显示等待，已有的坏邀请仍交给完整校验并明确失败。路径继续经过排他检查，旧邀请和旧输出不复用

确认下一局后整体替换 OnlineRound，清除音乐、反馈、Results、source clock、seq 和水位；新 Prepared 建立新 epoch，主机生成新的证书与邀请能力。一次确认只消费一份配置，按住或同批第二次确认不能穿透过渡阶段；最后一局完成后不再显示下一局入口

实时命令 / 事件队列各最多 256 项，入队与轮询不阻塞界面，满队列明确失败而不丢事实；每条线上 Facts 可以容纳 1–64 项，当前窗口按已经捕获的事实逐项提交。真实输入交换从预约起点起最多歌曲长度加 60 秒，Prepared 后本机 Ready 最多等待 120 秒

可在联网命令末尾加 `--live-observation NEW_DIR` 取得原生窗口、实际 Kira 游标、逐帧 CSV、输入计时诊断及 Running / 终态 PNG。此入口在完整品牌动画后注入明确标记的合成 Start / Hit，并自动关闭；多局时在 Finished 截图保存且下一局菜单实际可用后注入 Restart，每局写入 `round-1/`、`round-2/` 等独立目录，顶层 `summary.json` 记录各局 epoch 与状态，单局保留原顶层格式。它验证软件生产接线，不提供物理键盘、手柄、扬声器同步、双机或真人证据

2026-10-07 原生软件验证已由两个持续运行的游戏进程连续完成两局，epoch 分别为 `3124893908200008438` / `1601151596960184129`，双方事实计数分别为 `[35, 36]` / `[36, 36]`，每局均得到相同的 5 个事件；新证书、独立输出、旧局文件保留、seq 重置及 Receive → Join 均通过检查，见 [综合软件观测](../testdata/synthetic/session-diagnostics-observations-20261007.json)。真实设备、双机和真人体验仍待验收

## 加速历史运行

在两个终端运行，替换为已有的包与模板路径，邀请文件和输出目录必须是源包外的新路径，其父目录必须已存在

```sh
cargo run --locked -p cocobeat-lab -- net-host PACKAGE HOST_REPLAY.json 127.0.0.1:0 INVITE.json HOST_OUTPUT
cargo run --locked -p cocobeat-lab -- net-join PACKAGE GUEST_REPLAY.json INVITE.json GUEST_OUTPUT
# 客机未预装内容包时，接收至父目录已存在的新路径
cargo run --locked -p cocobeat-lab -- net-receive NEW_PACKAGE GUEST_REPLAY.json INVITE.json GUEST_OUTPUT
```

host 在写好邀请后输出 `INVITING`，包含实际端口、公开证书指纹与新 epoch；本机命令允许端口 0 自动分配，跨机器需选择可直达的本机单播地址及实际可用端口，当前没有 NAT 穿透或中继

模板可以使用 Replay v1 / v2、完整包身份和 `duo-watermark-v1`，先经原 core 校验；host 取 P1 子序列，join 取 P2 子序列，保留原 seq、整数 SongTime 与玩家内顺序，明确绑定到此次邀请的新 epoch。Hit 限定 `[0, canonical_frames)`，选中玩家最后一项必须是 `canonical_frames + confirmation_delay_frames + 1` 的显式水位；不在断线或 EOF 时补水位

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

实时 Running 阶段由原 worker 每秒进行一次独立的 56 字节 `CBMC` probe / reply / confirmation，双方保留同一 round 的四个实际时间戳；host 只接受与其实际回复匹配的 confirmation，guest 的第四时间戳来自本端实际收到 reply。它与初始及续演的 48 字节 `CBCK` 探测分域，共用唯一 datagram reader，原可靠输入读保持到完整帧结束

每轮维护限 250ms、最多 64 个入站包，每个 epoch 最多 1024 轮；轮次跨一次认证续接保留。初始化仅启动等待期限，ClockSync 仍未校准；丢失、重复和过期回复不能刷新样本。维护以 host 的实际第二时间戳 / guest 的实际第四时间戳计算本端年龄，超过原 2 秒有效期只通过 typed Stale 进入一次续演，其他非法状态仍终止；已用掉续演后再次失效终止。Runtime 保存实际 exchange 并保持 freshness 查询约束，新一局重建状态

[维护软件观察](../testdata/synthetic/live-clock-maintenance-observations-20261008.json)记录 600 轮仿射时钟控制、实际 loopback、可靠 FIFO 与真实 UDP 黑洞恢复；它更新进程时钟映射，双音源相位比较见下节，长期校正与完整故障矩阵继续开发

## 原音源相位维护

protocol / ALPN v8 在原 Running worker 内按歌曲进度约每5秒比较两个原音源，在可靠流传递原 generation、source_id、sequence、f64 position bits 与完整 publication 时间区间；每轮绑定一份实际 CBMC 四时间戳，不以时钟区间中点或推算游标代替原出版

实际2ms只读采样窗口最多64条，原出版年龄50ms、收集窗口1秒、可靠交付250ms；共同过去点须被完整不确定性区间夹逼，两个位置与差值全区间必须支持原±2400帧 guard。普通检查保持 Playing / Running 和输入，仅实际超 guard 才进入原句柄暂停与自然追赶流程；后验须使用更新的 CBMC 和实际 source 出版，不能靠 Armed 入队确认恢复

每轮沿原30秒期限，最多128轮 /64次校正，可靠 Phase 控制每方向每轮最多16条、每epoch最多2048条 /32MiB；原输入 FIFO、水位、End/count/EOF 与 FinishAck 保持，控制 Ended 只排空控制 reader，不替代歌曲 End。校正期间屏蔽演奏、歌曲与设置操作，取消和窗口关闭有效，重新开放输入需要新的释放事件

[相位维护软件观察](../testdata/synthetic/source-phase-maintenance-observations-20261009.json)包含42项 net /179项 runtime 普通测试、三个明确真实 QUIC 窄测及当前原生双方两轮实际 Kira 检查；双方自然完成、846条事实和5个事件一致，两个完整差值区间均为[-514,514]帧。该样本未触发校正，实际暂停追赶闭环、多轮强制漂移、active Phase 的认证续接重绑、设备与双机仍待验证；当前未支持的 active Phase 续接保持失败与真实前缀

## 身份与输出

每次 host 生成新自签证书、256 bit 随机邀请能力和随机 epoch，协议为 `cocobeat-session/8`，旧协议邀请拒绝。客户端先校验邀请内部的证书 BLAKE3，再使用标准 TLS 1.3 信任验证，并在发送能力 secret 前核对远端 leaf DER 完全相等；公开指纹不代替邀请能力，邀请应通过双方认可的渠道传递

邀请严格限制为 16 KiB，证书最多 4 KiB，不接受未知字段或版本；Unix 新文件权限为 0600，Windows 继承实际父目录 ACL。命令状态与 Replay 不输出 token 或私钥，首次连接仍只接受一次尝试；已开始的实时局另有下述独立续演能力，初始握手失败后重新运行会生成新邀请

协议 v8 延续 v5 的完整会话身份，包含必须出现的 `stage_compiler_version`，headless 使用明确的 `null`，实时使用当前共享版本 3；预装客机在 Hello、接收客机在 Installed 完整比较，接收方在读取资源前拒绝不支持的实时版本。窗口得到 Prepared 后以实际 StagePlan getter 再核对，成功后才能发送本端 Ready；权威 Replay 的 stage 身份再次与会话比较

实时 Replay v2 从实际原生 StagePlan 记录版本，开发歌曲与 headless 没有 StagePlan，因此输出保持 core-only v1；headless 模板即便带明确视觉版本也只提供经过校验的事实，不把模板版本或新 epoch 解释为已经渲染历史舞台

每端输出目录保留实际 `live.replay.json` 和 `status.json`；主机在两侧完整历史结束后生成 `authority.replay.json`，客户端核对长度、哈希、身份、epoch、完整逐玩家子序列和全部 core 结果，保存成功后才发送应用 `FinishAck`

主机验证 Ack 后正常关闭；客户端观察该关闭后才报告 `COMPLETE`。发生错误时保留已经成功 ingest 的真实前缀，状态为 `FAILED`，或已验证权威 Replay 的 `AUTHORITY_VERIFIED_UNCONFIRMED`。传输 EOF 和 transport ACK 不等同于应用 FinishAck；通信中断时两端可能观察到不同完成状态，协议不保证分布式原子提交

已有输出不覆盖，源包内及父目录符号链接别名拒绝；保存失败作为错误返回，不能把未写出的文件报告为已保存。成功后的权威 Replay 可以交给 [JSONL 诊断](replay-diagnostics.md) 检查实际 Hit、判定与水位

## 限额与边界

消息使用 4 字节大端长度和严格 JSON，单条最多 16 KiB，每批 1–64 个事实，单玩家输入累计最多 32 MiB；加速历史队列最多 4 批，实时命令 / 事件队列各最多 256 项，可靠流采用背压，队列压力不能静默丢输入。资源流采用固定 16 字节类型 / epoch 头和固定顺序的四个原始对象，不接受对端文件名；单对象上限依次为 512 MiB、16 MiB、4 MiB、64 KiB，总量最多 532 MiB + 64 KiB，传输缓冲为 64 KiB。权威 Replay 沿用现有 20 MiB 上限，收发窗口与固定 stream 数另有限制，datagram 只承载时钟探测，收发缓冲各 4 KiB

首次等待 guest 最多 120 秒，TLS / capability 阶段绝对限时 10 秒，资源传输异步等待最多 5 分钟，单次读写进展各 30 秒；ClockSync / Ready / ScheduleStart 屏障合计最多 30 秒、无输入进展各 30 秒，从预约起点开始的加速会话最多 15 分钟，最终 Replay 传输校验 60 秒、Ack 30 秒、端点关闭最多再等 5 秒。初始握手、资源与结束阶段超时终止当前会话；实时局的可恢复 Deadline 进入一次续演，原连接的半条消息不迁移到新连接

这些限制约束协议字节、事实与异步等待，不能抢占同步文件访问、哈希、音频解码或 core 运算，也不代表达到最大事实量时仍有合理帧时；网络压力、平台设备和人体体验须按各自证据评估。包发布复用同文件系统 staging → 校验 → rename，已有目标包括符号链接拒绝，失败只清理当前调用创建的对象；本地其他进程在最终存在性检查后创建空目录的竞态仍沿用现有发布实现，rename 不提供平台专用的排他替换保证

net 仅依赖 schema / core / replay / media 与网络实现库，core 不认识 Quinn；runtime 依赖 net 的同步有界 worker 入口，Tokio 和 QUIC I/O 留在其拥有的线程

## 同 epoch 续演

协议 v8 的实时局在双方已开始、均未 End、原进程与音乐实例仍存活时，允许一次 30 秒内的续演；传输 TimedOut / Reset、可靠输入帧 / 伙伴进展 Deadline 和 typed 时钟维护 Stale 可触发恢复，应用取消、失焦、队列满、非法事实和结束阶段失败仍为终态。客机回到原受信端点，主机复用原证书；独立的 256 bit 随机续演能力仅留在内存，绑定原内容、epoch、角色和 attempt 1，初始邀请不能代替它

恢复先冻结本地演奏输入，暂停同一个 Kira SoundHandle，保留 PCM、source generation / id、Session、core、seq 和全部原 Replay。两个实际 Paused publication 的前进 sequence 与相同 frame 确认稳定暂停后，worker 在独占 `recovery-1/` 保存自己的已接受 tape、GUI 全历史和恢复元数据；原录制文件保持，token 不进入日志。两端对账完整逐玩家历史，原 owner 前缀必须逐项相同，只有真正缺失的尾部经原 ingest 入口补入一次，重复 seq、倒退水位、改写与重排均终止

双方暂停帧为 `s_i`，共同目标 `R=max(s_i)`；在原进程时钟坐标的未来 `T`，各端以 `T-(R-s_i)/48000` 预约原句柄恢复。Armed 只证明预约入队，真实 Playing acknowledgment 且 frame 大于暂停帧后才恢复本地 AudioClockBridge，继续发送实际 catch-up 水位。新的 ClockSync 与原 source 连续 publication 围住固定的过去检查点 `T+100ms`，每端位置区间需在预计位置的 ±2400 帧内，跨端最坏区间相差最多 2400 帧；缺样本、过期、倒退、换源或回到 Paused 都失败，检查点不能延后以迁就结果

恢复期间一个拥有生命周期的只读线程每约 2ms 尝试读取同一音乐句柄的真实 source publication，独立校验所有读取的 sequence、位置、身份与时间区间；收到 RecoverySampling 的真实 `not_before` 后才保留最多 64 条门槛样本，避免提前 catch-up 耗尽窗口容量。音乐控制与读采样共享短锁，音频 callback 不取得此锁，品牌音效保持原控制路径；锁忙或 source 发布忙返回不可读，超过原 50ms 年龄、窗口超容量或线程失败均终止。线程固定在检查点后 80ms 完成，成功、取消、故障和窗口关闭都回收并 join；主线程继续检查当前源与状态并处理原时钟和事实，历史门槛样本不代替当前音乐状态。raw PlaybackState 与 coherent source snapshot 是独立读取，不宣称原子配对

只有双方真实 publication 验证与可靠 Live 确认通过后，窗口才从 Recovering 返回 Running。恢复期间仍接收原伙伴事实，屏蔽 Hit、歌曲和设置操作；菜单主控的 Esc / 手柄 East / Start 可以终止，关闭窗口仍有效，失焦也终止。恢复完成重建键盘与手柄释放屏障，按住的键不能穿透；`controls_enabled` 仍表示品牌开场总开关，恢复输入门控由阶段、菜单过渡与取消路由共同完成

原流与续接流各自每方向最多 32 MiB，长度前缀在 body 分配或发送前预留预算；只能续接一次，因此每方向累计最多 64 MiB。候选认证最多四次，未认证候选失败不损坏仍可用的原连接，已认证续演失败或 30 秒耗尽则终止。单条消息、队列、事实、Replay 和历史对账沿用有界限制；跨 QUIC 流的控制 Live 与 peer Facts / End 在 gate 完成前按可靠输入流顺序保留，不能提前结束或丢弃已经接受的事实

这次 gate 约束的是过去的软件 source publication 区间，尚不提供未来 callback 上界、声卡漂移补偿或扬声器同步保证；真实双机、物理输入、输出设备及长期漂移继续单独验收

## 故障后的新轮次软件验证

协议 v5 / Replay v2 接线完成后，127 项 runtime 测试、9 组实际 LiveSession loopback 及 3 组原生双轮检查通过；每组使用两个持续运行的游戏进程，分别覆盖正常完成、Ready 前本地 worker 取消和命中后本地取消 / 伙伴连接丢失，再使用新邀请完成第二局

Ready 前失败时客机尚未初始化网络 epoch，主机已有邀请 epoch；命中后两侧只保存各自实际收到的不同前缀，没有权威 Replay、补造 EOF 水位或假报 COMPLETE。第二局使用新 epoch、从 seq 0 开始，缺失包重新 Receive，已发布包完整校验后 Join 复用，旧文件保持，线程和保存屏障由生产路径消费

[网络观察清单](../testdata/synthetic/network-reentry-observations-20261007.json)绑定固定 debug 游戏二进制、419 项构建输入及独立依赖图谱，保留 22 张 agent 目检图和 6 张主线程补审；构建含当时未提交的音频 API / catalog，后续文案变更与源码临时改复分别记录，不声称最终提交不可变或连续源码未改。此结果是合成输入和 loopback 的原生软件验证，同 epoch 续演、物理输入 / 扬声器、双机与真人仍另验

## 有限软件源的多轮相位校正

2026-10-09，固定原600秒四对象包与原 protocol8 public worker，声明整数 active-time 软件源分别以 +1000 / -1000ppm 和相反方向运行；双方均实际完成 round4 /8 /12 三次校正，原差值完整区间不在 ±2400 帧内时触发，使用严格更新的真实 CBMC 和原 raw source floors 校验，所有后验完整区间回到原 guard，原30秒轮次期限及160 /220 /235秒源 /worker /root预算保持

两端每次冻结的逐玩家 Replay 前缀、owner count 和最终 Hit0..3 连续序列保持，源 generation /id、累计 active_ns、原小数余量和预约自然前进保留；独立复核重算60份原 proof 的四时间戳、整数 drift、capture bracket及完整区间。第三个负控向真实 worker 提交变化的 source identity，主机精确拒绝且零 Ready；旧负控因拒绝后继续发 Fact 先行 panic 的 FAIL 原记录保留，新 QA 只停止后续发送并读取真实 Failed

三个 case 均明确以有限 QA 取消结束，保存 FAILED 和部分 Replay，没有 End、补造最终水位、自然 EOF 或权威 Replay；它们验证真实 TLS /QUIC 控制与声明的软件积分源，不替代强制 Kira 漂移、CPAL /DAC、protocol9 active续接、双机或设备验收，见[多轮软件源观察](../testdata/synthetic/source-phase-model-observations-20261009.json)
