# MIR onset 时间根因诊断

这是隔离研究工具，复用 [原始基准](../mir-onset-probe/src/main.rs) 的匹配器、门槛和回归测试，读取原始基准落盘的 PCM，不修改原始 FAIL 或产品依赖；固定结论和产物身份见 [观察清单](../../testdata/synthetic/mir-onset-diagnostic/observations-20261003.json)

## 源码根因

检查对象为锁定的 Apache-2.0 `oximedia-mir 0.2.1`，依赖的源码 SHA-256 与行号保存在观察清单，来源为本机 Cargo registry 的真实实现

- `src/utils.rs:58–72` 只计算完整的左对齐 Hann 窗，尾部不足一个窗的样本不参与分析；Hann 首尾权重为零
- `src/beat/onset.rs:43–49` 把强度峰下标乘 hop 当时间，即返回窗起点，不能直接代表窗内的实际攻击位置；诊断给每个原始预测附上实际支持窗口及窗内最大能量差所在帧，该字段仅解释支持范围，不参与新候选输出
- `src/utils.rs:255` 的峰选择排除首尾位置；`src/beat/onset.rs:109` 的 adaptive threshold 没有被调用
- 同库公开的 `onset_peak::OnsetPeakDetector` 实际计算每块平均平方能量的正向差，并非 FFT 谱通量；该实现同样舍弃不足 hop 的尾部、排除峰数组首尾

## 受控候选

共对比五条路径：原 `beat` API 固定 window=1024 / hop=128；原 `onset_peak` API 的 hop=128、64；保留其 flux 与 adaptive median 的端点及尾块适配，分别使用 hop=128、64

适配仅补算原 PCM 中不足 hop 的最后一块，并允许峰数组的首尾位置参与相同局部最大值判定；时间戳始终是实际块起点，没有补 PCM、裁切、事后平移、回看真值或合成 confidence；最小峰距统一为 384 PCM 帧，median window=11 块、sensitivity=1.5，64 帧候选对应 1.33 ms 的块分辨率；改变 hop 也改变 11 块阈值上下文的物理时长，结果不是单独归因于分辨率的消融实验

两条适配路径均通过原 5 类 / 10 声道门槛；原 energy API 仍漏首尾，原 beat API 仍为 8 FAIL / 2 静默 PASS；原始 120 BPM、137.5 BPM 和 first-last 的 64 帧适配 median 绝对误差分别为 0、20、0 帧

额外保留 8 个构造控制样本：覆盖所有 128 个 hop 相位的脉冲、幅度降至 1/100 的同组脉冲、partial-hop 最后一帧、单样本文件、恒定电平、固定种子 xorshift32 噪声、440 Hz 持续正弦及 9973 Hz 持续正弦；脉冲真值来自独立的生成位置，后三种持续信号只有文件起点的构造攻击，其内部不定义额外 onset，这不是人类音乐标注

相位扫描、低幅度扫描和尾部控制均通过；64 帧适配对噪声产生 102 个额外峰、两种持续正弦各 100 个额外峰，单样本文件没有足够 median 上下文而漏检，恒定电平的短尾因浮点累计差出现一个额外峰；这些 FAIL 均保留，当前候选不足以作为生产音乐分析器

## 复现

先按 [原始基准说明](../../docs/mir-onset-probe.md) 生成五组 fixture；下面的输出目录必须尚不存在，从仓库根目录执行，缓存齐备时可加 `--offline`

```sh
mir_manifest=tools/mir-onset-diagnostic/Cargo.toml
mir_build=target/mir-onset-probe-build
cargo fmt --manifest-path "$mir_manifest" -- --check
cargo test --locked --manifest-path "$mir_manifest" --target-dir "$mir_build" -j1
cargo clippy --locked --manifest-path "$mir_manifest" --target-dir "$mir_build" --all-targets -j1 -- -D warnings
cargo build --locked --release --manifest-path "$mir_manifest" --target-dir "$mir_build" -j1
"$mir_build/release/cocobeat-mir-onset-diagnostic" target/mir-onset-probe-20261003 target/mir-onset-diagnostic-reproduction
```

最后一条命令预期退出 1，须同时核对 `report.json` 的 `status=FAIL` 和各项结果以区分质量失败与 I/O 错误；工具软件测试、Clippy 和构建通过，不代表所有候选质量通过；首轮探索结果 `results-v1` 保留在 target，最终交付使用 `results-v2`

新增 PCM 与构造标签使用 CC0-1.0，生成器源码使用项目 MPL-2.0；通过公开 API 复用上游计算，未引入新的第三方包版本；真实音乐、人类标注、canonical Ogg 回读和 Anchor 可玩性均为 NOT RUN
