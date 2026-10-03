# 单边频谱 HFC 候选

隔离研究工具，读取 [原始 onset 基准](../mir-onset-probe/src/main.rs) 和 [时间诊断](../mir-onset-diagnostic/README.md) 已落盘的同一批 PCM，直接复用原始匹配器及其测试；不改旧报告、旧源码或产品依赖

## 固定算法与坐标

原生 `oxifft 0.4.2` FFT → 单边 Hamming 幅度谱 → `oximedia-mir 0.2.1` 的 `onset_strength::OnsetDetector(HfcEnergy)`；参数在首次检测前固定为 window=128 帧、hop=64 帧、threshold_factor=1.5，窗口分辨率为 375 Hz；上游 peak picker 使用 ±8 个分析位置的均值和 ±2 个位置的局部最大值

FFT 完整输出只取 `0..=N/2`，包含 DC 与偶数窗的 Nyquist；若将实信号镜像频谱一并输入，`k` 与 `N-k` 的 HFC 权重和恒为 `N+2`，会抵消主要的频率区别；运行前以同能量的 1500 Hz 与 7500 Hz 两个正弦核对这一点，输出单边 HFC 及完整镜像谱对照值

每个窗先记录真实 PCM 区间 `[start,end)`，预测映射为离散支持区间中心 `ceil((start+end-1)/2)`；正常 hop 没覆盖 EOF 时，补上以 EOF 结束的真实 128 帧窗，因此最后一步可能少于 64 帧；所有 raw HFC、窗区间、映射坐标、原始 API `time_s` 和 peak 标记均保留，`time_s` 仅作为 API 原值而不代替实际窗坐标

不足 128 帧的文件使用唯一的真实短窗，不添加虚构 PCM；此时频率分辨率随真实窗长变化，且上游只有一个强度值时，1.5 倍局部均值门槛会使非零攻击漏检，这一失败保留；同样保留等高局部峰可能被重复选择、最后不规则步长改变局部时间范围的限制

没有补零、裁切、根据真值调参或平移结果；窗中心是分析坐标定义，不保证等于真实 onset，仍须接受原始 ±480 帧匹配容差、median 绝对误差≤96 帧、全匹配且零额外预测的门槛；原始强度和归一化强度均不视作 confidence

## 固定批次结果

2026-10-03 软件验收 PASS，候选整体质量 FAIL，逐例真值、指标、输入与源码哈希见 [观察清单](../../testdata/synthetic/mir-spectral-probe/observations-20261003.json)，完整窗级输出在 `target/mir-spectral-20261003/results-v1/report.json`

同能量频率自检的 PCM 能量差为 `1.11e-16`，单边谱 HFC 高/低频比值为 `4.19990`；完整镜像谱的两值则为 `2099.8406` 与 `2099.8083`，不能证明频率加权被正确保留

| 样本范围 | 结果 | 证据 |
| --- | --- | --- |
| 原始 5 类 / 10 声道 | 10 PASS | 120 BPM、137.5 BPM、first-last 的 median 误差分别为 0、14.5、63 帧，反相保持逐声道结果 |
| 128 相位扫描及低幅度扫描 | 2 PASS | 各 128 个脉冲全匹配、零额外峰，median 16 帧 |
| partial-hop 最后样本 | PASS | 预测 47937，真值 48000，误差 -63 帧 |
| 单样本文件 | FAIL | 漏掉唯一的 frame 0 攻击 |
| 恒定电平、440 Hz 与 9973 Hz 持续音 | 3 FAIL | 各零额外峰，但仍漏 frame 0 攻击 |
| 固定噪声 | FAIL | 一个额外峰，并漏 frame 0 攻击 |

上一批 64 帧 RMS 适配对噪声有 102 个额外峰、两种持续音各 100 个额外峰，本候选减少了这些固定控制的误报；首帧和噪声门槛仍未通过，不能宣布生产 MIR 准入；HFC 比较局部均值，在整个文件持续有能量时没有“文件前静默”的证据，不能用硬补 frame 0 的方式掩盖失败

本机 release 单次观察耗时约 0.214 秒，最大 RSS 91344 KiB，完整 JSON 约 10 MB；内存包含所有窗级 JSON 证据，不代表流式产品实现；真实音乐、canonical Ogg 回读、人工标注、Anchor 可玩性与跨平台执行均为 NOT RUN

## 复现

先按照两个前置工具的说明生成 clean 与 holdout PCM；以下命令从仓库根目录执行，输出目录必须尚不存在，缓存齐备时可加 `--offline`

```sh
mir_manifest=tools/mir-spectral-probe/Cargo.toml
mir_build=target/mir-spectral-probe-build
cargo fmt --manifest-path "$mir_manifest" -- --check
cargo test --locked --manifest-path "$mir_manifest" --target-dir "$mir_build" -j1
cargo clippy --locked --manifest-path "$mir_manifest" --target-dir "$mir_build" --all-targets -j1 -- -D warnings
cargo build --locked --release --manifest-path "$mir_manifest" --target-dir "$mir_build" -j1
"$mir_build/release/cocobeat-mir-spectral-probe" target/mir-onset-probe-20261003 target/mir-timing-20261003/results-v2 target/mir-spectral-probe-reproduction
```

存在任一质量 FAIL 时最后一条命令退出 1，须结合 `report.json` 区分质量结果和 I/O 错误；软件测试、Clippy 与构建分别验收

PCM 与构造标签沿用前两批的 CC0-1.0 来源，两个频率自检波形由本工具原创生成且同为 CC0-1.0，工具源码使用 MPL-2.0；只将已有的 Apache-2.0 `oxifft` 列为直接研究依赖并开启 `streaming`，没有引入新第三方包版本
