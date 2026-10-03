# 正向谱变化证据门槛

本工具在[冻结的原生 Flux 候选](../mir-flux-probe/README.md)输出处加一个过滤条件，复用其 FFT、真实 PCM 窗、原生峰选择和原始 Matcher；只读取上一批落盘的全部 24 项离散输入与 1 项连续观测，不生成新音频，不接生产 MIR

## 根因与固定适配

128 帧 Hamming 窗下，实正弦的正负频率窗口谱互相干涉，其幅度谱随窗口相位变化；已有 440 Hz / 9973 Hz 持续音的 Flux 假峰并非单纯浮点噪声，原生选择器仅比较局部均值的 1.5 倍，没有最小变化证据要求；按全文件最大值归一化同样不能去掉纯相对小峰

原始绝对 Flux 下限也不适合本轮：已有弱脉冲强度 0.1196–0.325 与高频持续音假峰 0.242–0.290 重叠；采用固定的谱幅值变化比例可以保留整体增益不变性

令 `F_i` 为原生正向谱幅值差，`S_i` 为真实单边谱的幅值总和，`D_i = max(S_j, j ∈ clipped[i-8,i+8])`；只保留满足 `D_i > 0 && F_i / D_i >= 0.5` 的原生峰，时间坐标不变

0.5 表示新增谱幅值至少达到邻域最大幅值总和的一半，属于预先固定的强变化研究标准，不是能量比例或置信概率；归一化复用原生选择器的 ±8 窗范围，规则 hop 下两端中心相距 21.33 ms，包含完整支持窗时覆盖 24 ms，减少相邻低频相位低谷对分母的影响

[运行前声明](../../testdata/synthetic/mir-flux-gate-probe/declared-experiment-20261003.json) SHA-256 为 `5361353670d6d2d6647030d45afe16f571d8f8c33f5e9179cabf922762904939`；128 帧窗、64 帧 hop、Hamming、原生均值系数 1.5、归一化半径 8、门槛 0.5 均不随样本或真值调整

## 验证契约

每项先重跑完整原生 Flux，与冻结报告逐项核对预测、支持区间、峰 flag、原生 f32 强度和 API 时间戳；谱幅值总和经复用 FFT 的第二次遍历取得，不改原生 analyzer；筛选后预测必须是原预测的有序子集，坐标完全相同，标签只在分析和过滤完成后进入原 Matcher

全量窗级对照只写入工作区报告，Git 中的观察清单保留逐例指标、固定参数和文件哈希；原报告的 24 个严格评分原样执行，连续 fade 的 truth 和 metrics 继续为 null；首帧漏检不能靠补候选修复，全部质量 FAIL 和新增漏检均保留

相同 f32 字段按原生位模式比较；新算的 f64 指标先通过与冻结 JSON 相同的序列化和解析路径再严格比较，消除文本解析表示差异，没有使用 epsilon 或修改指标

## 已知适用边界

过滤只能删除候选，不能恢复原生首帧零值造成的漏检；近邻的强事件会压制较弱事件，未来窗口也参与背景参考；渐变起音、低对比度叠加音符和密集节奏可能达不到门槛，短窗相位变化也可能继续超过门槛，这些限制在运行前声明中已固定

真实音乐、独立人工标注、校准 confidence、canonical Ogg 回读和生产接入仍需后续验证；新工具使用 MPL-2.0，复用的合成音频和构造标签沿用 CC0-1.0，第三方包沿用已有 37 个精确锁定版本

## 首次固定运行结果

2026-10-03 软件验证 PASS，整体质量仍 FAIL；[紧凑观察清单](../../testdata/synthetic/mir-flux-gate-probe/observations-20261003.json)记录全部逐例指标，完整窗级对照位于 `target/mir-flux-gate-20261003/results-v1/report.json`

| 范围 | 原生 Flux | 固定归一化过滤 |
| --- | --- | --- |
| 原 18 项严格工程控制 | 11 PASS / 7 FAIL | 11 PASS / 7 FAIL |
| 6 项有前史的离散音色控制 | 1 PASS / 5 FAIL | 6 PASS / 0 FAIL |
| 全部 24 项离散控制的额外峰 | 537 | 0 |
| 连续 fade 观测峰数，不计 F1 | 125 | 0 |

440 Hz / 9973 Hz 起音、等能量换音、噪声 burst、稀疏 kick-like 与 snare-like 均保留原坐标的正确匹配；整数和非整数脉冲、反相声道、全部 128 种脉冲相位及弱增益版本、partial-hop 尾部均未退化

7 项 FAIL 为 first-last 的两个声道、单样本、恒定电平、持续噪声及两个持续音的文件首帧事件；两个持续音原先在 frame 192 / 256 产生的内部波动峰落入 ±480 帧匹配容差，过滤后对应的两次匹配也被删除，新增 2 个 false negative；其余已匹配事件全部保留，不能把这两项写成“没有新增漏检”

原生 25 项的全部预测、44,417 个窗的支持区间、f32 强度与时间、峰 flag 及评分逐项重现，过滤后保持原预测子集，没有移动坐标；首次运行耗时约 0.515 秒，最大 RSS 232832 KiB，含冻结报告及全量 JSON 对照，不作为产品性能指标

## 复现

先按前置工具生成冻结 Flux 报告及其 PCM；从仓库根执行，输出目录须尚不存在，依赖已缓存时可加 `--offline`

```sh
mir_manifest=tools/mir-flux-gate-probe/Cargo.toml
mir_build=target/mir-flux-gate-probe-build
cargo fmt --manifest-path "$mir_manifest" -- --check
cargo test --locked --manifest-path "$mir_manifest" --target-dir "$mir_build" -j1
cargo clippy --locked --manifest-path "$mir_manifest" --target-dir "$mir_build" --all-targets -j1 -- -D warnings
cargo build --locked --release --manifest-path "$mir_manifest" --target-dir "$mir_build" -j1
"$mir_build/release/cocobeat-mir-flux-gate-probe" target/mir-flux-20261003/results-v2/report.json target/mir-flux-gate-probe-reproduction
```

任一离散质量项失败时退出 1 并写出完整 `report.json`；软件验证与质量结果分别报告
