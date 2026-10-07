# 01 · 计时实验

前置：00；已接入 Bevy/Kira，当前软件检查和真实设备实验分别记录

- [x] 独立 MonotonicTime/DeviceTime 与 ClockBridge 映射、有限历史、溢出和外推边界，7 项软件测试通过
- [x] 暂停、恢复、重启、设备丢失与校准失效的软件状态测试通过
- [x] 提供 `timing-sim` 的独立整数真值、误差统计和卡顿对照，独立测试通过
- [x] 完整 30 秒/64 秒/5 分钟/10 分钟软件模拟矩阵已运行：16 个情景、198,816 个采样、0 个超出声明的不确定性
- [x] Session 捕获时刻与消费时刻分离、Replay 一致性及游标停滞/倒退的软件集成测试通过；真实输入计时仍待设备验收
- [x] `audio-probe` 显式点击音工具已通过完整编译、Clippy、参数/信号/统计测试；源码 `d1290a2` 的 30 秒真实 Kira/CPAL 软件游标观测退出 0，见 [验收记录](../docs/testing.md#真实音频后端游标观测)，物理输出延迟、loopback、听感与校准仍为 NOT RUN
- [x] 显式本机 Replay 计时采集保存真实 Hit 观察 / 消费时刻、当次 mapping anchor 与来源，独立有界 source / callback 历史及原字节绑定；core 9 项、replay 11 项、runtime 174 项、lab 32 项与两组原生软件回路通过，详情滚动与原报告兼容见[计时观察](../testdata/synthetic/timing-sidecar-observations-20261008.json)
- [ ] Kira 定时点击与 loopback/硬件测量真实输出，导出偏移、漂移与 p50/p95/p99
- [ ] 真实播放中的卡顿、暂停、重启、设备切换及校准失效验收
- [ ] 在 Windows/Linux 分别测量键盘和手柄，记录 USB/蓝牙连接方式与最早可观察事件时间

当前游标 `±50 ms` 与规则窗口是未实测的实验配置；完整软件检查见 [验证策略](../docs/testing.md)，硬件条目保持 NOT RUN

退出条件：有可复现实验命令和带设备/构建信息的真实报告，明确表示分辨率、软件假设与实际精度，再依据测量调整判定窗口

2026-10-07 · Kira 公共 hook 的软件 callback / source publication 已接线：4 项 MockBackend 检查、Linux 原生 16 份 callback / 8 份 source 快照通过，见[观察记录](../testdata/synthetic/audio-publication-observations-20261007.json)。代次、source 身份、发布区间、年龄和失效分开处理，不把前一完整 callback 帧数当未来 deadline、设备延迟或声卡漂移界；原硬件和长期条目保持未完成

2026-10-08 · 本机软件计时 sidecar 已接线，接口与保存边界见[时间契约](../docs/timing.md#本机-replay-软件计时)；默认关闭新采集、旧 CSV 保留，显式采集不改变 ClockConfig / 原规则。只记录软件消息及独立历史快照，物理设备、声学输出、长期漂移和真实输入条目继续未完成
