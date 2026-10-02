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

运行时使用 Kira 音频回调最近发布的源播放位置建立观察，重复读取未推进的游标不会延长校准有效期；它不是声音到达扬声器的时刻，当前 `±50 ms` 是软件游标的初始实验估计，不是实测误差上界或端到端延迟保证

键盘在 Bevy `First`、手柄在 `PreUpdate` 输入处理后读取消息并打时间戳，Session 另外记录消费时刻；Replay 配套 CSV 保留 `observed_ns`、`consumed_ns`、`song_frames` 与 `uncertainty_frames`，首次软件读取时间不能宣称为硬件按键时间

本地反馈在消费 Hit 时触发，不等待共享确认；水位只在当前输入队列消费后推进，并保留当前软件观察的误差余量；共享规则的初始确认窗口见 [gameplay.md](gameplay.md)

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

采集工具已实现，软件编译与检查已通过，真实音频探针尚未运行；真实 loopback/外部输出测量、Windows/Linux 键盘/手柄延迟、听感与校准均为 NOT RUN，必须依据这些证据调整判定窗口与时钟参数
