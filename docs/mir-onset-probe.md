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

## 后续频谱候选

[单边频谱 HFC 候选](../tools/mir-spectral-probe/README.md) 继续读取同一批原始 PCM 和独立控制，原生 FFT 单边谱先通过等能量不同频率的加权核验；固定 window=128、hop=64、Hamming 窗与 1.5 倍局部均值阈值，预测映射到真实窗支持区间中心，未补 PCM 或平移旧输出

该候选的原 10 声道和 3 项控制通过，另 5 项控制仍 FAIL：持续音与恒定电平的额外峰为 0、噪声额外峰为 1，但这些持续信号的首帧攻击及单样本攻击仍漏检；逐项结果、坐标约定与冻结身份见 [谱候选观察清单](../testdata/synthetic/mir-spectral-probe/observations-20261003.json)，软件检查和独立审查通过不等于生产 MIR 准入，以上后续结果不覆盖本页原始基准结论

## 固定谱通量与音色控制

2026-10-03，[谱通量对照工具](../tools/mir-flux-probe/README.md) 沿用相同 PCM、单边 FFT、128 帧窗、64 帧 hop、1.5 倍阈值和真实窗坐标，比较原生 HFC 与 SpectralFlux；在首次生成和检测前冻结 6 个离散控制及 1 个连续渐变观察的参数、字面真值和指标边界，原 18 项及其全部 FAIL 保持

HFC 完全复现原 13 PASS / 5 FAIL，Flux 为 11 PASS / 7 FAIL；新增 6 项离散控制两者各 1 PASS / 5 FAIL。Flux 对等能量换音通过，但 9973 Hz 静默后起音产生 105 个额外峰，持续 440 / 9973 Hz 音产生 129 / 141 个额外峰；HFC 对 9973 Hz 起音通过，440 Hz 起音的 160 帧偏差超过 96 帧 median 门槛。平滑渐入渐出没有唯一离散真值，只记录 HFC 0 峰与 Flux 125 峰，不纳入 F1 或通过率

3 项软件测试、格式、Clippy、release 构建与独立复算通过，检测命令因质量 FAIL 退出 1；首轮跨 JSON 序列化比较 f32 强度时误用 f64 逐值比较的失败保留，修复为还原 f32 后逐位比较，没有改动质量容差、参数或标签。另复现上游重复 `pick_onsets` 不清旧标记的状态问题；当前离线管线只调用一次，因此该问题不解释本批质量失败

完整矩阵和身份见 [观察清单](../testdata/synthetic/mir-flux-probe/observations-20261003.json)。非零文件首样本不能揭示文件之前是静默还是持续录音，旧构造首帧标签继续参与工程评分，不自动视为可观测音乐起音；下一步集中验证谱泄漏、微幅谱变化与峰选择，真实音乐、最终 Ogg、人工标签和生产准入继续独立执行

## 固定谱变化过滤

[后续工具](../tools/mir-flux-gate-probe/README.md) 使用提前声明的局部谱幅值变化比例 0.5，只过滤既有 Flux 峰：新增 6 项音色控制全部通过，24 项离散控制的额外峰从 537 降至 0；旧 18 项仍 11 PASS / 7 FAIL，两个持续音原先误差容差内的假峰被删除后新增两次首帧漏检。完整原生窗口和指标均已复现，全部失败保留；近邻强弱、慢起音、真实音乐与编码回读仍待后续，未接生产 MIR

## 近邻与弱声部限制

同一过滤工具已加入预先声明的 9 项[后续控制](../tools/mir-flux-gate-probe/README.md#近邻慢起音与叠加声部)，保持全部原始参数与 Matcher：7 项离散控制为原生 2 PASS / 5 FAIL、过滤后 4 PASS / 3 FAIL，额外峰 229 → 0，漏检 2 → 3；两种慢起音没有唯一离散真值，仍只记录峰数，不计入通过率。旧 25 项的 44,417 窗完全重现，累计 31 项离散门槛为 21 PASS / 10 FAIL

384 帧近邻的弱峰在原生局部均值门槛阶段已被丢弃；3 kHz 弱声部叠加在 440 Hz 基底上时，frame 24000 的实际候选被全谱归一化过滤删除，独奏同一声部则通过。完整 [观察清单](../testdata/synthetic/mir-flux-gate-probe/observations-next-controls-20261003.json)保留匹配窗重叠的配对歧义和原始假峰，独立 PCM 重建及 FFT 复算通过；下一步分别修复两处压制机制，现结果仍不构成生产 MusicAnalysis、真实音乐或人工 Anchor 标签准入

## AudioFlux 原生候选

2026-10-07，[AudioFlux MIT C 候选](../tools/native-onset-check/README.md)完成 Linux x86-64 内置 FFT 构建、原 Matcher 单测及 12 项输入边界控制，29 份历史 PCM 按原 SHA 完整恢复；原 34 项矩阵为 2 PASS、28 FAIL、1 项短输入不支持与 3 项连续不评分，完整[观察记录](../testdata/synthetic/native-onset-observations-20261007.json)保留原始索引坐标、全部预测、近邻与首尾失败及未知置信度

默认峰选择窗口与 wait 保持，上述 hop 下最小候选间隔为 32 ms；原始 STFT 索引还存在超出 Matcher 的系统性提前，不把全部失败归因于近邻限制，不平移输出或放宽 ±10 ms / median≤2 ms 门槛

首次 sandbox LeakSanitizer ptrace 环境失败保留，随后宿主使用同一二进制与 detect_leaks=1 完成全部 controls / matrix，无 ASan / UBSan / LSan 诊断；33 份完整预测与 native 逐字节相同，1 项拒绝相同，指标一致。该固定组合质量继续 FAIL，未接入产品；同源 canonical、独立数值 oracle、四目标、长期成本及真人标签仍 NOT RUN
