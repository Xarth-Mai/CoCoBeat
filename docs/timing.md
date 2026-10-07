# 时间契约

`SongTime` 的一个单位是标准 48,000 Hz 音频的一个 **frame**，包含同一采样时刻的全部声道。双声道不会把时间单位翻倍。

| 含义 | frames |
|---|---:|
| 1 秒 | 48,000 |
| 1 毫秒 | 48 |
| 64 秒 | 3,072,000 |
| 2 秒预滚 | -96,000 |
| 10 小时 | 1,728,000,000 |

使用有符号 i64，支持预滚。`from_seconds` / `from_millis` 是整数精确转换；加帧和求差都检查溢出，失败返回 None。
`try_from_seconds_f64` 仅用于导入/UI 边界，取最近帧，半帧远离零；NaN、无穷和超出范围返回 None。
f64 不能精确表示整个 i64 范围，不能用显示值往返恢复原始帧；保存、判定与回放保留整数。

`MusicalTick` 是音乐网格位置，需要明确的 tick 分辨率和 tempo map 才能映射到 SongTime；`SessionEpoch` 是会话代际标识，重启时使旧事件失效。二者不隐式转换成 SongTime。

20.833 微秒只是一个帧的表示分辨率，不是输入到声音的端到端精度。禁止把 Bevy 帧数、f32 秒、包到达时刻当成音乐真值。

## 当前软件时钟

runtime 的 ClockBridge 已实现独立 MonotonicTime / DeviceTime、音频位置观察、有限历史映射与显式失效状态；它以每次观察为起点按标称 48 kHz 外推，把观察误差、配置的漂移上限和帧量化纳入不确定性，并不拟合真实设备漂移

默认最多外推 250 ms、假定漂移不超过 1,000 ppm、保留 256 个观察；暂停后恢复需要新观察，设备丢失与重启使用新的 epoch，过期或不连续会明确失败；这些状态已有纯逻辑测试，真实设备切换验收仍为 NOT RUN

运行时轮询 Kira 的公开 source position，并以本帧单调读取时刻建立 ClockBridge 观察；重复读取未推进的游标不会延长校准有效期，捕获查询保留当时实际命中的历史 anchor，而非保存时的最新观察。该映射不以 source publication interval 或 main-mix callback timestamp 代替原 anchor；当前 `±50 ms` 是软件游标的初始实验估计，不是实测误差上界或声音到达扬声器的时刻

键盘在 Bevy `First`、手柄在 `PreUpdate` 输入处理后读取消息并打时间戳，Session 另外记录消费时刻；Replay 配套 CSV 保留 `observed_ns`、`consumed_ns`、`song_frames` 与 `uncertainty_frames`，首次软件读取时间不能宣称为硬件按键时间

本地反馈在消费 Hit 时触发，不等待共享确认；水位只在当前输入队列消费后推进，并保留当前软件观察的误差余量；共享规则的初始确认窗口见 [gameplay.md](gameplay.md)

## 本机 Replay 软件计时

```sh
cargo run --locked -p cocobeat-game -- --timing-diagnostics
cargo run --locked -p cocobeat-game -- --package PACKAGE --timing-diagnostics
```

`--timing-diagnostics` 仅显式启用正常本地、package、library、authored import 或邀请制游戏；观看 Replay、退出式校验和 smoke 不接受此开关。默认仍保存既有 Replay / CSV，不采集新的稀疏音频历史或生成 timing sidecar；每次进程完整播放品牌开场并停 Ready，新确认才开始歌曲

开关在新会话、重新开始、返回主菜单和换歌后继续生效，但新 Session 使用自己的 epoch、内容身份、空 Capture / audio history；本地双人声明 P1 / P2，邀请会话只声明本进程实际本地席位。每份 sidecar 只含本进程接受的本地 Hit，不发给 peer，不把双方 process clock 拼接，不绑定网络 worker 的另一份 authority Replay

接受 Hit 时保存原软件消息 `observed_ns`、实际 `consumed_ns`、来源 `keyboard_message / gamepad_message / internal`、不确定性、1-based 原 Replay `fact_index` 及当次 ClockBridge 查询实际采用的 mapping anchor；持键屏障、菜单拒绝或曲外输入不凭空产生 Capture。时间来自同一 InputState 单调 origin，软件消费等待为 checked `consumed_ns - observed_ns`，不代表硬件按下时刻、声卡或扬声器延迟

显式模式在 Starting / Running / Pausing / Paused / Recovering / Finishing 的 GUI 更新中最多每 50 ms 读取一次历史音频快照，不补齐帧卡顿期间的缺口，不在音频线程分配、写盘或另开采样线程；每份最多 12,000 条 AudioRead，到上限停止额外读取并标为 `limit_reached`，原 Hit 与其 mapping 诊断仍继续记录

AudioRead 保留实际 read 区间及独立可空的 source / main-mix callback；缺出版、读取繁忙、失效或无法转换到本 origin 的字段保留 `null`，不填零。source 使用自己的 generation / source_id / sequence / position / publication interval，callback 使用自己的 generation / sequence / observed / previous batch，二者无需相同 sequence，也不承诺来自同一次 backend callback；previous_frames 只描述前一完成的 main-mix 批次，不是未来 deadline 或 DAC timestamp

保存时先沿原入口写 Replay / CSV，再为该 Replay 新建 `session-*.timing.json`；sidecar 绑定精确 Replay 字节 BLAKE3、完整内容 / 规则 / 原 build / 明确 nullable Stage、epoch、canonical_frames 与声明的本地席位。普通有界文件上限为 128 MiB，Capture 不超过 Replay 的 160,000 facts，未知字段、未知版本、错误身份或不一致关联拒绝，不覆盖旧 sidecar

sidecar 验证、创建、写入或 sync 失败时保留已成功写入的 Replay，并报告 `Replay saved at …, but timing sidecar failed: …`；既有 CSV 独立失败也保留 Replay。只有全部保存步骤成功才更新保存状态，失败仍可从原界面读取原因并重试，不能把 sidecar 失败显示为原 Replay 已丢失或已完整关联

本批 core 9 项、replay 11 项、runtime 174 项、lab 32 项与边界工具 4 项检查通过；冻结 447 项输入的实际 game / Lab、两组原生软件计时回路及 32 条 CLI 消费完成，原生详情三张图的指定范围目检通过，命令、身份、原始失败及补验见[验收记录](../testdata/synthetic/timing-sidecar-observations-20261008.json)。物理键盘 / 手柄、USB / 蓝牙、DAC、扬声器和真人计时体验继续 NOT RUN，记录方法不改变现有 ClockConfig 或规则窗口

## 模拟与待测证据

```sh
cargo run --locked -p cocobeat-lab -- time-smoke
cargo run --locked -p cocobeat-lab -- timing-sim
```

`time-smoke` 只核对整数单位；`timing-sim [output-dir]` 对 30/64/300/600 秒执行四种确定性软件情景，导出带 `SIMULATED` 标记的样本、绝对误差 p50/p95/p99、残余漂移与不确定性越界数，默认目录为 `target/timing-sim/`，独立真值与参数见 [实验说明](../testdata/synthetic/README.md)

## 显式音频采集

```sh
cargo run --locked -p cocobeat-lab -- audio-probe 30 target/audio-probe-30
```

`audio-probe <30|64|300|600> <output-dir>` 只在显式运行时播放对应时长的点击音，需要可用音频输出；目标目录可已存在，但不能含同名证据文件，命令不会覆盖已有实验

输出 `expected-clicks.csv` 保存源音频预期落点，`cursor.csv` 保存单调读取区间、真实 Kira 软件游标、更新间隔和相对软件漂移，`metadata.txt` 保存设备/构建信息、完成或失败状态及游标更新间隔分位数；这些记录始终标明 `physical_output_latency=NOT MEASURED` 和 `loopback_recording=NOT CAPTURED`

采集工具已通过软件编译与检查；2026-10-02，源码 `d1290a2` 的 `audio-probe 30` 在真实 Kira/CPAL 后端退出 0，ALSA `default` 为 48 kHz 双声道，记录 28,425 次软件游标观测和 2,811 个更新间隔，见 [验收记录](testing.md#真实音频后端游标观测)；默认 PipeWire 输出在运行前后均为静音、音量 30%，未作修改，CPAL 默认设备不等同于已确认的物理 USB 声路；真实 loopback/外部输出测量、Windows/Linux 键盘/手柄延迟、听感与校准仍为 NOT RUN，判定窗口与时钟参数仍需依据这些测量调整
