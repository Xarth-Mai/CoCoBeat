# 有限数值域与全频带组合诊断

本工具只复用四个已经冻结的 sign-kernel 源，确认 High 重采样幅度证书与 fullband residue 配置能在固定 q10 候选中组合；正式 vendor、profile 和 `[-1,1]` guard 保持原样，结果不构成生产准入

## 固定范围与复现

输入来自 `target/resample-l1-domain-20261003/{8000,44100,96000,192000}/source.wav`，四段均为项目原创、双声道反相、幅度 `32700/32768`、长一秒的确定性构造；对照是同一冻结目录中有限域诊断的旧频带输出，不是正式 guard 接受的输出

实现复用 `fullband-regression/probe.py` 的指标和 `oxideav/probe.py` 的限额/Ogg 检查，候选及媒体代码使用已冻结的 `e1b3a26` 快照，避免与正在开发的工作区混合；脚本核对两份旧 `FROZEN.json` 与 28 个输入/对照文件的 SHA-256，然后在输出目录复制隔离源码

唯一 codec 行为组合为既有 max-abs coupling、q10、`VORBIS_Q_SCALE=30`、residue end `1760→2048` 和仅用于诊断的 finite-only guard；脚本清除其余 `VORBIS_*` 参数，附加只读数值断言与峰值记录，不进行 clip、normalize、平移对齐或参数搜索

```sh
python tools/canonical-audio-probe/numeric-fullband/probe.py target/numeric-fullband-new
```

需要既有两批冻结 target 证据、已缓存 Cargo 依赖、NumPy、GNU timeout 和开发期 FFmpeg；输出目录必须为空，编码与每次回读各限 30 秒、2 GiB 地址空间，构建单独限 300 秒；每个源只编码一次，rate 1 仅用于核相位证书，不增加第五个音频样本

这是保留了历史冻结目录的工作区复现入口，不支持 fresh checkout 单命令重建；四个 sign-kernel WAV 的生成器 `target/resample-l1-domain-20261003/run.py` 与其 `kernels.csv`、核诊断源码仅存在工作区，没有等价的已跟踪完整生成入口，已跟踪的 `rusty-candidate/domain_probe.py` 只提供其中的 WAV 写入 helper

| 所需冻结目录 | 本轮直接读取 | 来源与生成边界 |
| --- | --- | --- |
| `target/resample-l1-domain-20261003/` | 四个 `source.wav`、finite-only 旧频带 Ogg/PCM/报告、`kernel/` manifest/lock 与上游源码前缀 | `kernel/src/main.rs` 导出 `kernels.csv`，工作区 `run.py` 用 `sign(coeff) * 32700/32768` 生成反相 stereo 源并调用 `target/media-finite-domain-final-20261003/finite-only/target/release/cocobeat-rusty-candidate`；其完整重建还依赖该旧诊断二进制 |
| `target/fullband-regression-20261003/` | `snapshot/`、`fullband/`、`strict-reader` | 已跟踪 `tools/canonical-audio-probe/fullband-regression/probe.py` 由 `e1b3a26` 导出源码再构建，完整矩阵本身还依赖它的 README 所列旧 target 证据；本轮只复制已冻结输入，不重新运行该矩阵 |

上述两份 `FROZEN.json` SHA-256 分别为 `3a89feec13c3062d1615172f3e5579b65d07c0d4e7f3c6d8e34feb23df4c7c2c`、`09c3f20472f3ef52df8abc60b827010a73d8dd30328a0103408293db1e7fca18`；缺失任一依赖时本工具会失败，不下载、不自动再造源，也不替换成其他输入

候选先执行自己的完整 reader，再使用冻结的 exact-stereo strict reader 和 FFmpeg native Vorbis decoder 独立回读；检查完整 48000 帧、有限 PCM、strict 与候选 PCM 字节一致、双 decoder 最大差不超过 `1e-6`，保留 Ogg CRC/序列/EOS、两声道误差、峰值、overs、首尾、源峰前后窗口、20.625 kHz 分界的频带能量及资源记录

## High 核的幅度证书

`P` 指源解码器交给重采样器的实际有限 f32 样本峰值，容器格式名义上的满幅不能替代这个前提；有损解码源可能超过 1，本批证明只使用 `P≤1`，四个构造源更小

核来自 OxiMedia 0.2.1 的实际 `resample.rs`，原始文件 SHA-256 为 `fe796ee4ebb67ce290adda9c11f98425ba3080c53e480049376980e9dfacfd48`，诊断只向 target 副本追加代码；High 为 192 taps、256 倍相位表，截止系数为 `0.945 * min(48000/source_rate, 1)`，表中每行归一化不代表其 L1 小于等于 1

精确实数下，相邻表行的凸插值 L1 不超过两行 L1 最大值；`kernel_bound.rs` 同时计算实际存储行的外包 L1，并通过上游 `fill_scratch` 枚举真实浮点插值后的系数，后者覆盖插值舍入

对每个相位的实际系数 `c[i]`，按实际点积顺序构造正向外包，`RN` 为 round-to-nearest

```text
b[0] = 0
p[i] = next_up_f64(RN64(P * abs(c[i])))
b[i+1] = next_up_f64(RN64(b[i] + p[i]))
f = RN32(b[192])
若 f < b[192]，返回 next_up_f32(f)，否则返回 f
```

三角不等式及舍入单调性使递推包住实际乘加的绝对值，f32→f64 输入转换精确、历史与尾部零填充不增加峰值；证书绑定本机实际生成的表、标准 IEEE 基本运算和无 fast-math 重排，不声明其他平台 libm 生成表的逐位一致性；JSON 将最终 f32 界转换为 f64 后输出，保留其精确数值

所有 `1..47999 Hz` 源共用一张表，约分后分母 `48000/gcd(rate,48000)` 整除 48000，rate 1 的 48000 相位枚举覆盖这个区间；48 kHz 直接旁路，幅度不变；96/192 kHz 各自只有一个可达相位，但表不同，其他降采样率未证明

| 实际 decoded 源峰值 `≤1` | 浮点输出绝对值上界 |
| --- | --- |
| `1..47999 Hz → 48000 Hz` | `2.757214069366455` |
| `48000 Hz` 旁路 | `1` |
| `96000 Hz → 48000 Hz` | `2.225487232208252` |
| `192000 Hz → 48000 Hz` | `1.9629822969436646` |

## 编码器预算与实际观察

证书中重采样输出均小于 3，编码器采用更宽的 `|PCM|≤4` 二进制包络推导算术余量；4 是本次条件式证明前提，不是已落地的新 guard，也不表示超过它必然失败

`codec_numeric.rs` 检查实际窗口和全部 long MDCT twiddle 有限且绝对值 `≤1`、q10 固定调参、正 floor 表下界 `>2^-24`、实际码本数值绝对值 `≤2047`、维度和码长 `≤32`、long residue partition 32/end 2048

| 阶段 | 条件式保守绝对值界 |
| --- | --- |
| 加窗输入 / f64 fold / pre-rotation | `2^2 / 2^3 / 2^4` |
| 512 点 FFT 的 9 级 / post-rotation / MDCT f32 输出 | `2^22 / 2^23 / 2^14` |
| 单声道 residue / coupling residue / VQ 差值 | `2^38 / 2^39 / 2^40` |
| 32 项平方和 / RD cost | `2^85 / 2^86` |
| psy band energy / spreading sum | `2^39 / 2^49` |
| 对数均值 / geometric power / flatness 比值 | `2^5 / 2^47 / 2^87` |

FFT 每级实际不超过输入分量界的三倍，使用可精确表示的四倍端点外包，随后乘精确 `1/512`；码本逐标量贡献按八次 cascade 外包并逐次 next_up；由此得到基本运算的有限余量，不能仅凭最终输出无 NaN 宣称整个 codec 普遍安全

`codec-budget.json` 中 f32 字段采用 Rust 最短往返十进制展示，不能把其十进制文本当作精确的实数上界；例如 cascade/VQ 差值的实际 f32 分别为 `549756338176`、`549756403712`，均严格小于已断言的精确端点 `2^40`；核证书和 14 阶段峰值另外转换为 f64 后输出，没有此展示精度差异

psy 的 `ln/exp/powf/log10/sqrt` 另有范围假设：在已界定的有限正区间上返回符合这些粗界的值；实际四例在 clamp/默认选择掩盖异常之前记录 band energy、对数均值、exp、flatness、spread、VQ 成本等 14 个阶段，均有调用且在界内；class 0 分区能量没有逐项插桩，其有限性由 coupled residue 界及固定 32 项预算保证

插桩保持原算术表达式和样本值是源码审查结论，本轮没有额外运行未插桩组合对照来证明编码字节同一性；解码器只报告本轮完整回读证据，不将上述编码器推导推广为全部解码路径的证明

## 2026-10-03 结果及下一步

四例全部完成，输入 resampled PCM 与旧冻结输出逐字节相同，strict 与 FFmpeg 各返回 48000 帧，最大 decoder 差 `4.76837158203125e-7`，首尾 2048 帧误差均为 0

| 源率 | 旧有限域、旧频带 SNR L/R dB | 有限域 + 全频带 SNR L/R dB |
| --- | --- | --- |
| 8 kHz | 18.3134 / 22.7844 | 18.3134 / 22.7844 |
| 44.1 kHz | 4.4202 / 4.5233 | 15.3114 / 16.8951 |
| 96 kHz | 1.2891 / 1.3675 | 10.6000 / 12.2470 |
| 192 kHz | 1.1480 / 1.2132 | 8.8041 / 10.0461 |

全频带降低这些病理宽频瞬态的高频损失，但没有消除量化/长窗误差；96/192 kHz 低频误差接近旧值，192 kHz 左声道低频误差能量仍有约 `6.7e-8` 增量；44.1/96/192 kHz 输出超过 unity 的样本数增加，完整数值保留在观察文件中，不拿整体改善掩盖回退或扩展到音乐听感结论

这批证据支持下一步将候选数值域写为明确的实际 PCM 幅度合同，使用解析预算配合核证书；不支持把 guard 简单换为 `is_finite()`、以一次 1.16 峰值设上限，或把 `<3` 的局部核界推广到所有降采样率/所有 finite 源；生产仍保留原 guard，后续需补全部承诺率的证书或清晰的超域拒绝，以及 profile 音质、seek、Windows/Linux 原生执行与人耳验收

本次保留两次执行前失败：核源码 hash 的分隔换行检查失败，修正后源身份恢复；随后候选构建日志成功但外层进程退出 143，未编码任何源，原因未确定；最终分阶段恢复、重新构建记录与唯一四次编码均保留，不将恢复前日志当作完整成功结果

完整冻结产物位于 `target/numeric-fullband-20261003/`，持久摘要为 `testdata/synthetic/canonical-audio-probe/numeric-fullband-observations-20261003.json`；源码/二进制/系数表身份覆盖追加的 `numeric.rs`、MDCT 常量检查和两级 Python helper，FFmpeg 仅用于开发期独立参照
