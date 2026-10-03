# 固定全频带回归

此开发期探针比较冻结 q10 候选与仅修改 long residue `end=1760→2048` 的 scratch 副本，正式 vendor、入口、输入域 guard 和生产 profile 均保持原状

```sh
python3 -B tools/canonical-audio-probe/fullband-regression/probe.py target/fullband-regression-20261003
```

需要 Linux、Rust/Cargo 离线依赖、Python/NumPy、GNU timeout、开发期 FFmpeg，以及先前 `target/codec-transient-20261003`、`target/rusty-candidate-q10-20261003`、`target/rusty-quality-diagnosis`、`target/resample-l1-domain-20261003`、`target/media-finite-domain-final-20261003` 的冻结证据；命令拒绝已有输出目录，不播放音频

源码从固定提交 `e1b3a261f1e4866dc377680f846225e75cf9b670` 归档，media/schema 与两个候选均在输出目录构建，两版 codec 仅 setup 的两个字节不同；清除继承的 `VORBIS_*` 后显式设置 `VORBIS_Q_SCALE=30`，q10、max-abs coupling、码本及窗口不调整

固定 19 例覆盖原 10 例、额外低音量/交换声道/同相/反相、首尾脉冲、两种采样率的域内瞬态及两种重采样超范围拒绝；原 64 秒原创音乐和含非静音尾部的 600 秒输入完整保留，后两例仅确认原 guard，不组合 finite-only 改动

每次编码、严格完整回读和 FFmpeg 独立解码保留 30 秒及 2 GiB 限额；`readback.rs` 直接调用 `decode_canonical`，帧数来自既知编码输入，两个变体共用同一个 reader，失败输出和日志为诊断证据而非有效音频

比较先检查完整长度，再计算从 frame 0 开始的逐声道同位置误差，不移动时间轴、拟合增益、裁剪或归一化；报告包括每秒、首尾 2048 帧、源峰前后窗口以及全长指标，静音或零误差 SNR 为 null，配套能量保留真实含义

频段能量通过最长一秒、无窗的各块 DFT 分别累加，使用 Parseval 权重，数组的两行分别为低于 20,625 Hz 和不低于该值，两列为左右声道；这是固定块谱诊断，边界泄漏仍存在，不等价于听感或响度

结构 PASS 不设置新的音质阈值，不代表音乐多样性、听感、seek、其他原生平台或生产准入；每例质量变化独立保留，报告中的资源为所记录进程及 harness 的限额观测，不用来跨批排名性能
