# MIR onset 合成基准

2026-10-03，`oximedia-mir 0.2.1` 的 `beat::OnsetDetector` 在本批原始 PCM 时轴上为 **质量 FAIL**；工具的单测、格式、Clippy 与 release 构建为 **软件 PASS**，两种结果分别记录于 [固定观察清单](../testdata/synthetic/mir-onset-probe/observations-20261003.json)

## 方法与门槛

[实验源码](../tools/mir-onset-probe/src/main.rs) 使用独立研究 workspace 与 [Cargo.lock](../tools/mir-onset-probe/Cargo.lock)，只启用 `oximedia-mir` 的 `beat` feature，不进入产品依赖；输入是原创 CC0-1.0、48 kHz 双声道 F32LE，写盘后重新读取各声道，检测器不接收真值标签

固定 window=1024 帧、hop=128 帧；因果脉冲从真值帧的 0.5 幅度开始，在 64 帧内线性衰减，文件最后一帧的脉冲只保留一个有效样本；120 BPM 与 137.5 BPM 输入的真值为独立字面帧表，其中后者逐点舍入 `24000 + n×230400/11`，不累计已舍入周期；静默无事件，反相样本左右声道独立检测，不混成单声道

API 返回的秒数保留原值，再按 `round(f64(seconds)×48000)` 转为帧；拒绝非有限、负值、乱序或越界预测，不裁开头、不补 padding、不移动时间原点，也不补造 API 未提供的 confidence

按时间顺序选取最早可行的一对一匹配，容差为含端点的 ±480 帧（±10 ms），有符号误差为预测减真值；本批要求所有真值恰好匹配一次、没有额外预测，且匹配误差绝对值的 median≤96 帧（2 ms）；静默单独要求零预测，零分母指标与没有匹配对时的 median/P95 均为 null

用户原始研究报告第 1854–1866 行规定 clean onset 的 F1@±10 ms≥0.98、median≤2 ms；**全匹配是本批更严格的门槛**，不是该报告的原文要求；这些是合成工程门槛，不是听感或真人 Anchor 准入标准，原报告身份与门槛分别保存在观察清单中

## 实测结果

共 5 个样本、10 个独立声道；每个样本的两个声道预测及指标相同，下表数字均按单声道列出

| 样本 | 帧数 | 真值数 | 预测数 | TP / FP / FN | F1 | 结果 |
| --- | ---: | ---: | ---: | --- | --- | --- |
| pulse-120 | 240000 | 8 | 8 | 0 / 8 / 8 | 0 | FAIL |
| pulse-137.5 | 240000 | 8 | 8 | 0 / 8 / 8 | 0 | FAIL |
| first-last | 48000 | 3 | 1 | 0 / 1 / 3 | 0 | FAIL |
| silence | 48000 | 0 | 0 | 0 / 0 / 0 | null | PASS |
| opposite-polarity | 240000 | 8 | 8 | 0 / 8 / 8 | 0 | FAIL |

120/137.5 BPM 脉冲按序比较时，预测分别偏前 640–704/611–727 帧；这只是观察相邻序号的诊断，不是容差内匹配，不能用这些未匹配配对计算正式 median 或 P95，也未用该偏差平移结果；所有非静默声道的正式 TP 都为 0，时间误差统计保持 null

`first-last` 的真值为 `[0,24000,47999]`，检测器只返回 `[23296]`，首尾漏检，中间也没有落入容差的匹配；反相两声道保持各自结果，不能由此泛化为任意混音或立体声鲁棒性

本次 release 单次执行耗时约 0.315 秒、最大 RSS 22116 KiB，实验进程虚拟地址空间限 2 GiB；这些是本机观察，不构成跨平台性能结论；单个自检覆盖字面真值、脉冲位置、重复预测、一对一匹配、容差端点、median 门槛与空指标，已有输出目录拒绝覆盖

## 复现与证据边界

从仓库根目录运行以下命令；首次获取依赖需要网络，缓存齐备可加 `--offline`，输出目录必须尚不存在

```sh
mir_manifest=tools/mir-onset-probe/Cargo.toml
mir_build=target/mir-onset-probe-build
mir_output=target/mir-onset-probe-reproduction
cargo fmt --manifest-path "$mir_manifest" -- --check
cargo test --locked --manifest-path "$mir_manifest" --target-dir "$mir_build" -j1
cargo clippy --locked --manifest-path "$mir_manifest" --target-dir "$mir_build" --all-targets -j1 -- -D warnings
cargo build --locked --release --manifest-path "$mir_manifest" --target-dir "$mir_build" -j1
"$mir_build/release/cocobeat-mir-onset-probe" "$mir_output"
```

本批最后一条命令预期退出 1，并保留 `report.json` 的 `status=FAIL`、每例 truth/predictions 与原始 PCM；退出 1 也可能来自用法或 I/O 错误，须结合报告和 stderr 区分，不能只看退出码判质量

完整 Rust 源码、manifest、锁文件、二进制、17 个产物与验证日志的 SHA-256 均保存在观察清单；原始结果位于 `target/mir-onset-probe-20261003/`，软件检查位于 `target/mir-onset-probe-validation/`，大 PCM、构建缓存及第三方源码不进入 Git

本批仅验证五类 raw PCM；canonical Ogg 回读、MusicAnalysis、AnchorCompiler、真实音乐、人类标注或听感、Windows/ARM 执行均为 NOT RUN；后续时间根因诊断不纳入这份原始基准

## 后续时间诊断

另行交付的 [诊断工具](../tools/mir-onset-diagnostic/README.md) 定位到左对齐窗起点被作为 onset 时间、峰选择排除首尾以及不足窗或 hop 的尾部被丢弃；原基准及上述 FAIL 保持不变

同库公开的能量差检测 API 加入端点和尾块处理后，64/128 帧 hop 两种配置均通过原 10 声道门槛，以及独立相位扫描、低幅度和 partial-hop 尾部控制；但持续正弦与噪声产生大量额外峰，单样本和恒定电平控制也有失败，因此仍未准入生产 MIR。参数、90 项逐例矩阵、软件检查与报告身份见 [固定观察](../testdata/synthetic/mir-onset-diagnostic/observations-20261003.json)，工具最终退出 1 并报告整体质量 FAIL
