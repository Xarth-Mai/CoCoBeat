# 真实谱通量与 HFC 固定对照

本工具复用 [上一批 HFC 候选](../mir-spectral-probe/README.md) 的原生 FFT、单边谱、真实 PCM 支持区间及中心坐标，并复用 [原始匹配器](../mir-onset-probe/src/main.rs)；只切换 `oximedia-mir 0.2.1` 的 `OnsetFunction::HfcEnergy` 与 `SpectralFlux`，不修改旧源码、旧观察或 vendor

## 运行前声明

[控制声明](../../testdata/synthetic/mir-flux-probe/declared-controls-20261003.json) 在首次生成或检测前冻结，SHA-256 为 `e26a7851c8769cb0666f813ed56c04f94b1255e26a0b35ba4fb577b90dae09ba`；两种算法均固定 128 帧 Hamming 窗、64 帧 hop 和 1.5 倍局部均值阈值，参数不根据真值或结果调整

原 18 项输入、标签和全匹配工程门槛全部保留；另生成 6 个离散构造控制：有前置静默的 440 Hz / 9973 Hz 起音、等能量 7500 Hz → 1500 Hz 换音、带平滑释放的噪声 burst、稀疏 kick-like 与 snare-like 音；事件帧使用预先列明的整数表，生成参数与标签分别保存在声明中，算法只接收落盘后回读的 PCM

第 7 个新增控制是持续 440 Hz 正弦的平滑渐入渐出，未指定唯一瞬时 attack，`truth_frames` 与 `metrics` 为 null；只观察预测数量、位置及原始强度，不能把它计入零真值 F1 或通过率；等能量换音的事件则是明确构造的 note switch，仍不代替人类对可感知攻击的标注

## 边界与指标解释

用户原研究报告第 1854–1866 行规定 clean onset F1@±10 ms≥0.98、median 绝对时间误差≤2 ms，并把真实音乐与合成工程门槛分开；本工具继续执行原始实验更严格的“所有真值各匹配一次、零额外峰、median≤96 帧”，没有删掉首帧控制或用宽松指标覆盖已有 FAIL，原报告身份见 [初始观察清单](../../testdata/synthetic/mir-onset-probe/observations-20261003.json)

报告第 1750–1757 行要求 leading silence、tonal onset、fade 与很短或截断媒体，却没有把任意非零文件首样本都定义为可观测的音乐 onset；一个从静默开始的持续音与从更长持续录音截下的相同 PCM，可能具有不同的边界事件语义，有限文件本身无法重建文件之前的历史；当前工程首帧标签依然参与原样评分，不能靠硬补 frame 0 或假定未知前史来通过

原生 SpectralFlux 在第一帧或频谱长度改变时返回 0，候选保持其实际行为；HFC 的单样本强度与局部均值相等，1.5 倍阈值必然拒绝，这属于当前局部证据规则的限制，不是数组越界或漏遍历首帧的程序错误；原始强度不作为 confidence

## 上游状态负例

独立负例先加入强度 `[0,0,1,0,0]` 并调用 `pick_onsets`，随后追加 `[100,0,0,0]` 再调用；同一完整序列在新 detector 中只选一次作对照

该 API 只把通过条件的 `onset_flag` 写成 true，重新计算时不清除旧 flag，导致旧峰低于新的局部阈值后仍保留；最小上游修法是在本轮比较前重置各 frame 的 flag，本工具只记录真实负例，不修改 vendor，实际离线候选总是在全部谱帧到齐后调用一次；这一状态问题没有参与上一批 HFC 的 5 个失败，也不能被当作它们的修复

## 固定批次结果

2026-10-03 软件检查 PASS，两种候选整体质量仍 FAIL；首次完整报告为 `target/mir-flux-20261003/results-v2/report.json`，逐项指标、原研究身份和产物 SHA-256 见 [观察清单](../../testdata/synthetic/mir-flux-probe/observations-20261003.json)

| 固定范围 | HFC | SpectralFlux |
| --- | --- | --- |
| 原 18 项严格工程控制 | 13 PASS / 5 FAIL | 11 PASS / 7 FAIL |
| 新 6 项离散音色控制 | 1 PASS / 5 FAIL | 1 PASS / 5 FAIL |
| 平滑渐入渐出连续观测 | 0 个峰 | 125 个峰 |

原 18 项 HFC 的预测、匹配指标及上游原生 f32 强度逐项重现；SpectralFlux 首帧零值使 first-last 的两个声道新增漏检；其持续 440 Hz / 9973 Hz 控制分别产生 129 / 141 个额外峰，未用“前史不可见”解释或豁免这些文件内部误报

新增等能量换音中，SpectralFlux 恰好匹配两个事件而 HFC 额外报一个峰，说明变化证据有独立价值；9973 Hz 起音只有 HFC 通过，440 Hz 起音的 HFC 定位晚 160 帧而超过 median 门槛；SpectralFlux 在这两个可见前史的起音上定位误差为 32 帧，却分别有 96 / 105 个额外峰；噪声 burst、kick-like 和 snare-like 仍产生额外峰，不能因为有前史便宣布问题已解决

上游状态负例实际得到 prefix `[2]`、重复选择 `[2,5]`、完整序列新 detector 单次选择 `[5]`；状态 bug 与候选音色误报分别保留，不把 API 状态修补说成分析质量改进

首轮 `results-v1` 在 HFC 回归核对阶段退出 101，尚未完成新控制的质量报告：旧 f32 强度经过 JSON 的 f64 十进制解析后出现一个 f64 ULP 差，直接比较 JSON Number 误报；随后改为恢复上游契约的 f32 并逐位核对，未使用 epsilon、修改参数或放宽质量门槛；首轮日志、源码与二进制身份保留，`results-v2` 预期质量退出码为 1

最终单次本机观察耗时约 0.565 秒、最大 RSS 258528 KiB，进程地址空间限制 2 GiB，完整窗级 JSON 约 27 MB；内存包含旧报告与两模式的完整窗级证据，不作为产品性能结论；当前原生短窗 SpectralFlux 不作为 HFC 的生产替代，下一步应针对实际音色的内部误报继续验证，真实音乐、人工标签、canonical Ogg 回读和跨平台执行仍为 NOT RUN

## 复现

先按三个前置工具生成固定的 clean、holdout PCM 与 HFC 报告；以下命令从仓库根目录执行，输出目录必须尚不存在，缓存齐备可加 `--offline`

```sh
mir_manifest=tools/mir-flux-probe/Cargo.toml
mir_build=target/mir-flux-probe-build
cargo fmt --manifest-path "$mir_manifest" -- --check
cargo test --locked --manifest-path "$mir_manifest" --target-dir "$mir_build" -j1
cargo clippy --locked --manifest-path "$mir_manifest" --target-dir "$mir_build" --all-targets -j1 -- -D warnings
cargo build --locked --release --manifest-path "$mir_manifest" --target-dir "$mir_build" -j1
"$mir_build/release/cocobeat-mir-flux-probe" target/mir-onset-probe-20261003 target/mir-timing-20261003/results-v2 target/mir-spectral-20261003/results-v1/report.json target/mir-flux-probe-reproduction
```

任一离散质量项失败时最后一条命令退出 1，须结合 `report.json` 区分质量结果与 I/O 错误；软件检查、质量门槛及上游状态负例分别记录；新增音频和构造标签使用 CC0-1.0，生成器源代码使用 MPL-2.0，沿用已有 37 个第三方包版本
