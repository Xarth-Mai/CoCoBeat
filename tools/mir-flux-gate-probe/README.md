# 正向谱变化证据门槛

本工具在[冻结的原生 Flux 候选](../mir-flux-probe/README.md)输出处加一个过滤条件，复用其 FFT、真实 PCM 窗、原生峰选择和原始 Matcher；默认模式读取上一批落盘的全部 24 项离散输入与 1 项连续观测，`--controls` 模式生成下述新控制；这两种模式共用原固定过滤器，`--band-candidate` 对同一批固定输入验证独立选峰候选，`--floor-candidate` 验证带分母下限，`--background-candidate` 验证归一化变化的时间背景门控；均不接生产 MIR

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

## 近邻、慢起音与叠加声部

[新增运行前声明](../../testdata/synthetic/mir-flux-gate-probe/declared-next-controls-20261003.json)在首次生成和分析前冻结，SHA-256 为 `3d74071f6c59bd82535b5b86300850d8a16ac7605850eebd450ce8c2b1d3dd31`；新增 9 段各 1 秒的 48 kHz mono f32 PCM，先落盘再读取，全部参数、原生峰选择、过滤门槛与 Matcher 保持不变

- 4 项近邻控制：同型 3 kHz 短衰减音，幅度 0.4 / 0.04、间隔 384 / 1536 帧，分别强→弱和弱→强；128 帧有限支撑在末尾仍有非零截断，截断若产生额外峰也照常评分
- 2 项慢起音：440 Hz、幅度 0.2，自 frame 12000 开始，用 1152 / 4800 帧半余弦包络升至全幅并持续到 EOF；攻击支撑区间是构造观察范围，未提供唯一感知 onset，truth 和 metrics 保持 null
- 3 项声部控制：440 Hz、幅度 0.2 的基底在 frame 6000 进入，3 kHz 新声部以幅度 0.2 / 0.02 在 frame 24000 进入，另有同一弱声部独奏对照；硬起点以 cos(0)=1 定义，叠加在 f64 完成后只转换一次 f32

384 帧间隔小于原 Matcher 的 ±480 帧容差，两次事件的匹配窗重叠；保留最早可行的一对一配对、全部构造真值和预测坐标，单凭配对不能判断哪个物理声部被保留。近邻同时影响原生均值门槛与过滤器分母，只有原生已发出而过滤器删除的峰才可归因于本过滤步骤

复现新增控制，输出目录须尚不存在；退出 1 仍表示离散质量门槛未全部通过，报告照常完整落盘

```sh
"$mir_build/release/cocobeat-mir-flux-gate-probe" --controls target/mir-next-controls-reproduction
```

### 新控制固定运行结果

[完整观察清单](../../testdata/synthetic/mir-flux-gate-probe/observations-next-controls-20261003.json)保留所有预测、逐项原始配对、误差和 PCM 哈希；全窗报告在 `target/mir-next-controls-20261003/controls-v1/report.json`。7 项离散控制为原生 2 PASS / 5 FAIL、过滤后 4 PASS / 3 FAIL，额外峰从 229 降至 0，但漏检从 2 增至 3；连同旧控制共 31 项离散门槛，过滤后 21 PASS / 10 FAIL，整体质量仍 FAIL

| 控制 | 原生坐标或峰数 | 过滤后坐标 | 过滤后结果 |
| --- | --- | --- | --- |
| 384 帧强→弱，truth 12000 / 12384 | 11968 | 11968 | FAIL，漏 1 |
| 384 帧弱→强，truth 12000 / 12384 | 12352 | 12352 | FAIL，漏 1 |
| 1536 帧，两种强弱次序，truth 12000 / 13536 | 11968 / 13504 | 11968 / 13504 | 两项 PASS |
| 1152 / 4800 帧慢起音 | 97 / 95 个峰 | 均为 0 | 不评分 |
| 等幅叠加，truth 6000 / 24000 | 111 个峰 | 6016 / 24000 | PASS |
| 弱声部叠加，truth 6000 / 24000 | 113 个峰 | 6016 | FAIL，新增漏 1 |
| 弱声部独奏，truth 24000 | 10 个峰 | 24000 | PASS |

近邻 384 帧的弱事件在原生选择器就没有对应候选，两项过滤前后输出完全相同，不能将这两次漏检归为归一化过滤造成。弱→强项原 Matcher 把 12352 配给 12000，得到 +352 帧误差，这保留了原匹配规则，也展示了重叠容差窗的归因限制

弱声部叠加在 frame 24000 确有原生候选，谱变化比例约 0.13956，固定 0.5 门槛将其删除；同一弱声部独奏在该坐标的比例约 1.00000 并通过，等幅叠加约 0.60748 并通过。弱叠加的原 Matcher 首先把较早的持续音波动峰 23552 配给 24000，因此原始匹配数本身也不代表准确恢复了声部进入；逐窗证据确认 24000 候选被过滤，保留这一新增漏检

5 项软件测试通过，其中 1 项为新增构造检查、4 项沿用已有或 include! 的前置检查；Clippy、release 构建和旧模式回归通过，旧 25 项的全部 44,417 窗与原报告在只去除计时字段后完全相同，原有 7 项过滤后失败未删除。实际新增采样约 0.064 秒，包含合成、落盘、回读、分析和 JSON 输出，仅为研究工具观测，不是产品性能验收

独立验证从实际 f32le 重建全部 432000 帧，逐样本位一致；另用 NumPy f64 FFT 与数学 Hamming 重算全部 6741 窗，谱通量最大绝对差约 1.15e-5，比例最大差约 4.87e-7，所有原生候选上的过滤去留与所有 Matcher 字段均复现。这是独立数值核对，没有宣称跨 FFT 的 f32 位一致或独立重现极小噪声的原生选峰

当前候选仍不能进入生产 MusicAnalysis。下一次修复应分别针对原生局部均值选择对近邻弱峰的压制，以及全频谱总幅值分母对低对比声部的压制，在预先固定的同一矩阵验证；直接调低 0.5 不能证明已解决持续音假峰。慢起音没有唯一离散标注，本轮只确认没有输出候选，不能将其记为准确检出或准确排除；独立人工标注、真实音乐、编码回读、beat/downbeat、confidence 与 Anchor 可玩性继续保留为待验收项

## 分频带局部变化候选

`--band-candidate` 实施针对近邻均值压制和全谱分母压制的单一修复候选，复用全部现有 34 项 PCM（31 项离散评分、3 项连续观测），不生成音频或改写旧报告；[运行前声明](../../testdata/synthetic/mir-flux-gate-probe/declared-band-local-20261003.json) SHA-256 为 `bdb0190741a83eb8c1fa1527719339f7ab1dc056bf43b7a74da9dacec8ed1219`，参数和输入身份在候选执行前冻结

128 / 64 Hamming 与真实支持窗保持原样，单边 bins 分为 `[0,2)`、`[2,4)`、`[4,8)`、`[8,16)`、`[16,32)`、`[32,65)`；每带复用原生 f32 正向谱幅值差和 `F`，质量 `S` 为该带幅值的 f64 和，分母 `D` 改为同带 ±2 窗的最大 S；`D=0` 时比例为 0，否则取 `F/D`，再令 E 为六带比例最大值

新选峰器要求 E 至少为 0.5 且为 ±2 窗局部最大，完全相等时只留该邻域最早窗；它独立选择真实支持窗中心，候选不再是旧原生预测子集。短窗裁剪频带到真实 bins，空带无输出，首窗原生 Flux 仍为 0；不加入能量 floor、epsilon、比例 clamp、平移、首帧补点或按样本调整的参数

每个输入的同一份加载样本同时用于重算原生 Flux、旧全谱过滤和新候选；原生与旧过滤的逐窗字段、预测和指标必须与冻结报告一致。完整文件 SHA-256 在独立执行器运行前后核对，单独调用 Rust CLI 不声称提供不可变文件快照

按前节构建工具后，以下复现命令检查全部参考报告与 PCM 身份，再执行唯一固定候选并再次检查，输出目录须尚不存在；退出码保留质量 FAIL

```sh
python3 - "$mir_build/release/cocobeat-mir-flux-gate-probe" target/mir-band-local-reproduction <<'PY'
import hashlib, json, pathlib, subprocess, sys
p = pathlib.Path('testdata/synthetic/mir-flux-gate-probe/declared-band-local-20261003.json')
assert hashlib.sha256(p.read_bytes()).hexdigest() == 'bdb0190741a83eb8c1fa1527719339f7ab1dc056bf43b7a74da9dacec8ed1219'
d = json.loads(p.read_text())
files = d['references'] + [d['upstream_source_reference']] + [
    {'path': i['pcm_path'], 'sha256': i['pcm_sha256']} for i in d['inputs']]
def verify():
    for item in files:
        assert hashlib.sha256(pathlib.Path(item['path']).read_bytes()).hexdigest() == item['sha256']
verify()
result = subprocess.call([sys.argv[1], '--band-candidate',
                          *(r['path'] for r in d['references']), sys.argv[2]])
verify()
raise SystemExit(result)
PY
```

### 分频带候选首次运行结果

软件验证 PASS，针对两种压制的修复验收与整体质量均 FAIL；[完整观察清单](../../testdata/synthetic/mir-flux-gate-probe/observations-band-local-20261003.json)记录全部预测、配对和退化，实际全窗报告位于 `target/mir-band-local-20261003/results-v1/report.json`，未进行参数扫描或二次调参采样

| 31 项离散控制 | 原生 Flux | 原全谱过滤 | 分频带候选 |
| --- | --- | --- | --- |
| PASS / FAIL | 14 / 17 | 21 / 10 | 17 / 14 |
| TP / FP / FN | 333 / 766 / 7 | 330 / 0 / 10 | 336 / 665 / 4 |

两种 384 帧强弱次序均恢复 `[11968,12352]` 并通过原门槛；弱叠加的 frame 24000 也恢复为候选，胜出带为 bins `[8,16)`、比例约 0.81194，但该案例还有 35 个额外峰，仍为 FAIL。6 项旧 PASS 退化：静默后 440 Hz / 9973 Hz 持续音、噪声 burst、kick-like、snare-like 和等幅叠加；不能据三处攻击附近出现候选就宣称修复完成

持续音回归的假峰显示低幅值质量旁瓣被逐带比例放大：440 Hz 持续音的 40 个新峰全部由 12–24 kHz 的 band 5 胜出；9973 Hz 持续音的 142 个新峰全部由 0–375 Hz 的 band 0 胜出。独立 f64 FFT 复算仍产生相同的全部 34 项预测列表，包括 665 个离散额外峰，因此本次回归不能仅归于原生 f32 舍入噪声

实际 PCM 的独立代表窗量化如下；S 占比为单边幅值总和之比，平方占比为未对内部 bins 加倍的单边平方幅值之比，两者都不冒充感知响度或 Parseval 时域能量

| 代表窗 / 胜出带 | S 带 / S 全谱 | 平方幅值占比 | F 带 | D 带 | F / D |
| --- | --- | --- | --- | --- | --- |
| 440 Hz，frame 1920 / band 5 | 1.1770% | 0.001285% | 0.0780894 | 0.1526605 | 0.5115235 |
| 9973 Hz，frame 576 / band 0 | 0.3152% | 0.001566% | 0.0185748 | 0.0247650 | 0.7500408 |
| 弱叠加，frame 24000 / band 3 | 6.0711% | 0.302427% | 0.9133437 | 1.1248915 | 0.8119393 |

前两项胜出带避开了主音的最大谱 bin，却因自身低质量旁瓣随窗口相位变化而通过相对门槛；第三项包含真实进入的 3 kHz 声部。该区别支持研究带有明确频谱尺度的分母下限，不能把前两项当作可直接忽略的浮点残差

FN 从 10 降到 4 也不表示可靠恢复了 6 个真实攻击：持续噪声的 frame 384、440 Hz 持续音的 frame 448 和 9973 Hz 持续音的 frame 128 恰落入原 frame-zero 标签的 ±480 帧容差，原 Matcher 将其配成 TP；这不能证明未知文件前史中的 onset，原有 7 项边界/持续信号工程案例仍全部 FAIL。三个连续观测只记录 37 / 29 / 27 个候选，truth 和 metrics 继续为 null

6 项软件测试通过，其中 1 项覆盖新的选峰、平台、空带/短窗、零分母、同窗多带、增益和坐标边界；Clippy 发现并移除了 `E>=0.5` 后冗余的 `E>0` 条件，未改数学规则或声明，原诊断保留。fmt、Clippy、release 构建通过，首次正式运行约 1.078 秒、最大子进程 RSS 503748 KiB，仅记录研究工具成本

独立核对全部 34 项 PCM 身份、51,158 窗和 306,948 个带窗口，旧原生与全谱过滤逐窗/预测/指标完全复现，新公式与全部 Matcher 字段也复算通过；另对实际 PCM 做 NumPy f64 FFT，34 项预测坐标全部一致，谱质量/Flux/ratio 最大绝对差分别约 1.59e-5 / 1.65e-5 / 2.21e-5，不宣称跨 FFT 的原生 f32 位或近似平局胜出带一致

本候选保留为失败的研究实现，不接入 MusicAnalysis；下一步只针对已有的低质量带放大问题形成有明确幅值尺度的归一化下限约束，再验证同一完整矩阵，不追加标签、缩小范围或降低质量门槛

## 带宽与局部谱峰下限

`--floor-candidate` 实施唯一 `band-peak-floor-v1`，复用同一 34 项实际 PCM、构造真值与 Matcher；[运行前声明](../../testdata/synthetic/mir-flux-gate-probe/declared-band-floor-20261003.json) SHA-256 为 `8cdf238f1428af2f1dd9ebda5f4735d98c6b618fdf60fbf35530f5c9236283c3`，在正式运行前经独立审查冻结，不新增质量样本或改写旧结果

令 P 为同一 ±2 窗内所有真实单边 bins 的最大原生 f32 幅值，n 为当前带实际裁剪后的 bin 数；唯一改动是将带分母设为 `max(D, n * 0.01 * P)`，其余 F、六带、128 / 64 Hamming、0.5 门槛、局部最大选择与时间坐标保持原样。完整窗 n 为 `[2,2,4,8,16,33]`；零谱、空带和不可观察的首窗不补候选

0.01 是根据已见开发回归设定的唯一工程常数，对应 -40 dB 幅值比例；带宽补偿使下限与带幅值总和具有相同单位，不宣称来自感知校准或外部最优参数。此下限与 F、S 在共同有限增益下按同一尺度变化，没有固定绝对电平门槛；不宣称任意 f32 增益下位一致

旧 band 与新 floor 共用一次 FFT 特征遍历，报告保留原 D、实际 bin 数、窗口/上下文谱峰、floor 项及最终有效分母；新 E 不大于旧 E，但局部排序可能变化，预测不保证是旧预测子集。每项同时重现旧 band、全谱过滤与原生 Flux 的全部窗、预测和指标，只排除计时字段

以下命令要求声明列出的原冻结报告与 PCM 已在对应路径；前节重新运行的报告包含新计时值，不能直接代替这些哈希固定的参考文件。验证覆盖 4 份参考报告与所有完整 PCM 的运行前后哈希，输出目录须尚不存在

```sh
python3 - "$mir_build/release/cocobeat-mir-flux-gate-probe" target/mir-band-floor-reproduction <<'PY'
import hashlib, json, pathlib, subprocess, sys
p = pathlib.Path('testdata/synthetic/mir-flux-gate-probe/declared-band-floor-20261003.json')
assert hashlib.sha256(p.read_bytes()).hexdigest() == '8cdf238f1428af2f1dd9ebda5f4735d98c6b618fdf60fbf35530f5c9236283c3'
d = json.loads(p.read_text())
files = d['references'] + [
    {'path': i['pcm_path'], 'sha256': i['pcm_sha256']} for i in d['inputs']]
def verify():
    for item in files:
        assert hashlib.sha256(pathlib.Path(item['path']).read_bytes()).hexdigest() == item['sha256']
verify()
result = subprocess.call([sys.argv[1], '--floor-candidate', d['references'][0]['path'], sys.argv[2]])
verify()
raise SystemExit(result)
PY
```

### 局部谱峰下限首次运行结果

软件验证 PASS，目标修复与整体质量仍 FAIL；[完整观察清单](../../testdata/synthetic/mir-flux-gate-probe/observations-band-floor-20261003.json)保留全部预测、配对、新峰与失败，正式全窗报告在 `target/mir-band-floor-20261003/results-v1/report.json`，仅运行一次固定候选

| 31 项离散控制 | 原生 Flux | 原全谱过滤 | 原 band-local | band-peak-floor |
| --- | --- | --- | --- | --- |
| PASS / FAIL | 14 / 17 | 21 / 10 | 17 / 14 | 21 / 10 |
| TP / FP / FN | 333 / 766 / 7 | 330 / 0 / 10 | 336 / 665 / 4 | 334 / 269 / 6 |

两种 384 帧近邻继续输出 `[11968,12352]`；弱声部叠加输出 `[6016,24000]` 且没有额外峰，三项均通过原门槛。原 band 的 17 项 PASS 全部保留，之前 6 项退化中恢复静默后 440 Hz / 9973 Hz 音和等幅叠加这 3 项；noise burst、kick-like 和 snare-like 仍分别有 70、34、31 个额外峰，目标修复门槛尚未满足

持续 440 Hz / 9973 Hz 的内部假峰全部清除，因而原 Matcher 曾将内部峰误配给 frame-zero 标签的两次 TP 也消失，FN 从 4 增至 6；未补首帧，原 7 项边界工程失败全保留。持续噪声仍有 134 个额外峰，frame 384 仍被 ±480 容差配给 0，不能视为首帧 onset 恢复；3 项连续观测均变为 0 峰，truth 和 metrics 仍为 null

所有新 E 都不大于旧 E，但 kick 的 47 峰变为 37 峰时出现了 21 个旧列表中不存在的坐标，其局部最大次序确有改变，全部新坐标已记录。剩余失败的首个额外峰胜出带中，原 D 已高于本次 floor：noise burst 的幅值占比约 4.82%，snare 约 6.36%，kick 约 94.33%；这些真实带变化不能统一归为低质量旁瓣或 f32 噪声，继续调本轮 beta 没有验收依据

7 项软件测试、fmt、Clippy 和 release 构建通过；新增窄测包含实际触发 floor 的 440 Hz cosine、0.5 倍增益、脉冲坐标、空带和单样本边界。运行前独立审查通过，唯一正式采样约 2.259 秒、最大子进程 RSS 1147812 KiB，包含冻结报告及全量 JSON，仅为研究工具观测

独立复算核对 34 项输入与 29 个 PCM 文件、4 份参考报告、全部 51,158 窗及 306,948 个带窗口，旧三基线和四路 Matcher 完整复现。另从实际 PCM 做 NumPy f64 FFT，34 项预测坐标全部相同，谱幅值和 / Flux / ratio 最大绝对差约 1.59e-5 / 1.65e-5 / 3.37e-6；不宣称原生 f32 位一致或近似平局的胜出带一致

本轮结果冻结为未准入的研究候选；剩余修复须解释噪声统计波动和打击衰减过程，同时保留已恢复的 8 ms 弱近邻。真实音乐、独立人工标注、canonical Ogg、beat/downbeat、confidence 和 Anchor 可玩性尚未验收

## 归一化变化的时间背景门控

`--background-candidate` 实施唯一 `band-background-excess-v1`，复用当前 floor 的全部特征与原峰；令 `B[i] = mean(E[j], j ∈ clipped[i-8,i+8])`，只有原峰满足 `E[i] >= B[i] + 0.5` 才保留。均值包含当前值与实际零值，边界按真实窗数计算；不对 E-B 重新选峰，因此新预测严格为旧 floor 有序子集，坐标不变

[运行前声明](../../testdata/synthetic/mir-flux-gate-probe/declared-band-background-20261003.json) SHA-256 为 `605f83ed62e7a9eb14e3d2905f7948bfe0dbb5b21f7d9bbf85c7a034a632ee4c`，保持原 34 项输入和 Matcher，复用半径 8 与门槛 0.5，参数不随样本调整。新增时间背景作用于已归一化 E，不继承原始强音的 10 倍幅度，也不设置 refractory；是否保住真实近邻仍由原完整矩阵判断

17 个 E 的中心相距 21.333 ms，计入每个 E 自带的 ±2 窗，实际内区依赖 21 个 FFT 窗、29.333 ms PCM，最远未来窗末端距当前中心 14.667 ms；这是离线分析，报告仍使用原支持窗中心。新增门控只读取已有 E，不增加 FFT、依赖或音频；噪声背景过高可能删除真实进入，低频衰减尖峰也可能继续超过均值，软件检查不能代替质量验收

运行要求声明中原冻结报告及 PCM 位于原路径，重新生成的报告有新计时值，不能替代这些固定哈希；运行前后检查 5 份报告和所有完整 PCM，输出目录须尚不存在

```sh
python3 - "$mir_build/release/cocobeat-mir-flux-gate-probe" target/mir-band-background-reproduction <<'PY'
import hashlib, json, pathlib, subprocess, sys
p = pathlib.Path('testdata/synthetic/mir-flux-gate-probe/declared-band-background-20261003.json')
assert hashlib.sha256(p.read_bytes()).hexdigest() == '605f83ed62e7a9eb14e3d2905f7948bfe0dbb5b21f7d9bbf85c7a034a632ee4c'
d = json.loads(p.read_text())
files = d['references'] + [
    {'path': i['pcm_path'], 'sha256': i['pcm_sha256']} for i in d['inputs']]
def verify():
    for item in files:
        assert hashlib.sha256(pathlib.Path(item['path']).read_bytes()).hexdigest() == item['sha256']
verify()
result = subprocess.call([sys.argv[1], '--background-candidate', d['references'][0]['path'], sys.argv[2]])
verify()
raise SystemExit(result)
PY
```

### 时间背景门控首次运行结果

软件验证 PASS，目标修复和整体质量均 FAIL；[完整观察清单](../../testdata/synthetic/mir-flux-gate-probe/observations-band-background-20261003.json)记录全部预测、原配对、275 个删峰和新增漏检，唯一正式报告位于 `target/mir-band-background-20261003/results-v1/report.json`

| 31 项离散控制 | 原生 Flux | 原全谱过滤 | 原 band-local | 原 floor | 背景门控 |
| --- | --- | --- | --- | --- | --- |
| PASS / FAIL | 14 / 17 | 21 / 10 | 17 / 14 | 21 / 10 | 20 / 11 |
| TP / FP / FN | 333 / 766 / 7 | 330 / 0 / 10 | 336 / 665 / 4 | 334 / 269 / 6 | 324 / 4 / 16 |

noise burst 恢复 PASS，全部 70 个额外峰被删除；两种 384 帧近邻与弱叠加继续通过。kick 保留三次正确攻击但仍有 3 个额外峰，坐标为 17984、41984、72000，其真实支持窗跨越既定音符的 18000、42000、72000 帧截断；snare 仍有 frame 45888 的额外峰，并删除了首次真实攻击的 frame 18048 候选

两项 128 相位脉冲控制各漏 4 次攻击，使原 floor 的两个 PASS 退化；被删峰 E 约为 0.51974 / 0.52448，背景约为 0.05882，未达约 0.55882 的固定门槛。snare 首次攻击 E 约为 0.72275，背景约为 0.28114，也未达约 0.78114 的门槛；这 9 次可观察攻击丢失构成实际退步，不能仅按额外峰下降宣布修复

新增第 10 个 FN 来自持续噪声的内部 frame 384 不再被旧容差配给首帧标签；该例现在没有候选，原 7 项边界仍全部 FAIL。所有新预测均为 floor 的有序子集，没有新坐标；3 项连续控制仍为 0 峰且 truth/metrics=null，均不作为 onset 正确性结论

8 项软件测试、fmt、隔离 Clippy 和 release 构建通过，声明与源码身份在运行前独立核对；本轮只执行一次正式质量矩阵，没有调参数重跑。约 3.636 秒、最大子进程 RSS 2033044 KiB 包含冻结输入报告和逐窗 JSON，仅为研究工具观测

独立核验 34 项输入、29 个完整 PCM 文件、5 份参考报告、全部 51,158 窗、旧四基线及五路 Matcher，通过逐窗原 E、背景 B、门槛、flag 和有序子集检查；从实际 PCM 用 NumPy f64 FFT 复算的新 34 项坐标列表全部一致，E / B 最大绝对差约 2.15e-6 / 3.58e-7，不宣称原生 f32 位一致。验证器使用与 Rust 相同次序的逐项 f64 累加，避免 Python 3.14 内置 sum 的不同累加策略产生 1 ulp 差；没有因此修改候选或加入 epsilon

该候选继续保留为失败的研究实现，生产 MusicAnalysis 不接入；后续修复必须同时保住脉冲相位和打击起音，解释仍保留的衰减/截断附近峰，并维持全部现有开发回归与独立音乐、编码和人工验收边界
