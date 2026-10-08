# 原生 beat / downbeat 实验候选

`cocobeat-lab import-experimental-beat SOURCE_AUDIO AUTHORING_JSON left|right NEW_BUNDLE` 是显式实验入口，沿用源快照、唯一生产编码器、严格最终读回和手工作者内容事务，再从同一最终 canonical 音频生成 beat / downbeat 候选；Ready 菜单及普通手工导入不运行此分析

```sh
ORT_DISABLE_TELEMETRY=1 /path/to/release/bin/cocobeat-lab import-experimental-beat /path/to/source.wav /path/to/authoring.json left /path/to/new-bundle
/path/to/release/bin/cocobeat-lab verify-package /path/to/new-bundle/package
/path/to/release/bin/cocobeat-lab inspect-stage /path/to/new-bundle/package 0
```

上例为 Linux 布局，Windows 的 `cocobeat-lab.exe` 位于发行根目录；输出父目录须已存在，NEW_BUNDLE 须尚不存在。成功输出 `package/` 的原四对象及独立 `evidence/`，游戏加载 `package/`，原始证据随 bundle 保留；作者 Anchor / 段落仍来自 authoring，不从模型结果自动生成 Anchor

候选 profile 为 `native-small0-high22050-f64fma-minimal-v1-candidate`：严格读回最终 48 kHz 双声道音频，High 重采样到 22.05 kHz，由用户明确选左或右声道，生成 128 mel 输入并执行固定 small0 ONNX 的 CPU 推理和 minimal 后处理。原 q、原始 logits、分块与聚合覆盖、最近 beat 的 downbeat 对齐及 `q × 960` 的 canonical 整数帧映射分别保留；越界 / 重复 / 非有限值拒绝，不能移动时间原点或补造首尾

MusicAnalysis v2 的 beat / downbeat capability 标为 `Candidate / Algorithm`，confidence 为 `None`；strength 和 downbeat_probability 是未校准的模型 / 适配分数。tempo / onset / repetition 在本入口为 Unsupported，手工 sections 与实测 energy 保留，`production_admission=false`；自动 Anchor 策略和音乐可玩性继续单独验收

产品直接加载固定 ONNX Runtime 1.30.0 CPU SDK 与模型，按编译进二进制的字节数、BLAKE3、目标架构和 tensor 契约核验，编辑 provenance 不能改变受信身份。Linux Lab 须位于 `bin/`，Windows Lab 位于包根目录，两者资源都从实际可执行文件的发行根定位：`lib/onnxruntime/` 和 `assets/models/beat-this/small0.onnx`；不依赖工作目录、产品 Python、运行时下载或 GPL-only 组件，资源缺少 / 损坏直接报告错误，普通手工导入是独立命令

返回错误时保留实际失败原因及本次独占 staging 的路径，失败 evidence 可能不完整；内层事务清理自己创建的四对象，未成功发布 NEW_BUNDLE 不作为可加载包。已有目标拒绝覆盖，检查和哈希不提供恶意并发路径替换的完整防护保证

本批 Linux x86-64 软件检查通过：media 46 项、xtask 5 项，native 3 项重复窄测、最终三包全目标 Clippy 和 463 项冻结输入构建；初 Clippy 的两处 `op_ref` 失败保留。四组实际导入共 12 条成功命令（4 import / 4 verify / 4 Stage），四组同一实际 native spect 的独立 QA logits / 聚合 / 原 q 对照最大误差均为 0；QA Python 只用于独立对照，不进入产品分析运行

13 项事务 / 资源控制为 5 成功、8 预期拒绝，覆盖过短音频、非法作者 / 声道、已有目标、缺失或损坏模型 / SDK；旧新 Lab 普通手工导入的四对象逐字节一致。固定 Lab SHA-256 为 `217d2c987b7dd3c2501abced6ae0f337f712ac79ea33f74784e538b8bf662f68`，命令、源码、原始结果与失败见[观察记录](../testdata/synthetic/native-beat-observations-20261008.json)及对应 raw index，工作区原图位于 `target/native-beat-delivery-20261008/`

2026-10-08 使用同一冻结 debug Lab 完成原创 64 秒 PCM 重复 9 次加首 24 秒的 600 秒成本样本：导入 exit0、293.164615 秒，kernel 单子进程峰值 RSS 1495372 KiB、/proc 采样峰值 1496700 KiB、实际采样最多 3 threads；独立 verify-package exit0、19.600872 秒，计时另列且四对象字节保持。实际 N=28800000、M=13230000、F=30001、21 chunks、原 q 范围与覆盖核对通过，481 项实际输入前后 hash 相同；这是 Linux x86-64 debug CPU 观察，不是发行 CPU/RSS 预算准入，完整命令、原始输入和结果见[十分钟观察记录](../testdata/synthetic/native-beat-cost-observations-20261008.json)及对应 raw index

48 bytes 上限控制仅有 1 frame 真实 payload，header 声明 600 秒加 1 frame，实际拒绝原因确为十分钟上限，因此只通过 declared-duration header guard；完整超长音频和本 600 秒样本的独立 Python 数值对照仍为 NOT_RUN。首轮源生成的最终 print TypeError 与 staging glob 漏记均保留，后续只读文件快照及 mtime 标记不能补成精确内部阶段计时；长样本仍为 Candidate / Algorithm / confidence None，成本样本不作为音乐真值

旧 profile 的 wholeSpect 19 PASS / 9 FAIL（整体 FAIL）及音乐质量 FAIL 全部保留；同输入推理数值一致不证明原生前处理等价官方音乐前处理，也不构成完整 MIR 准入。各目标原生发行接线与实际 tag Release 按[构建发行](build-release.md)逐版本另验；发行 CPU/RSS 预算、Windows 控制台取消、真实音乐 / 设备 / 真人验收仍为 NOT_RUN；Linux 实验导入取消与同进程 API fresh 重试按下文软件记录限定

## 原生候选工作台

```sh
cargo run --locked -p cocobeat-lab -- workbench-beats /path/to/bundle/package /path/to/bundle/evidence --locale zh-CN
```

`workbench-beats PACKAGE EVIDENCE [--locale CODE]` 在完整验证歌曲包后读取原生候选证据目录，核对 analysis 末尾生产注记绑定的 summary、固定 profile / 模型 / 音频身份、N / M / F / 声道、六项资源的字节数 / BLAKE3，以及原 aggregate、峰组、最近 beat 对齐与包 analysis 的一致性；缺少 / 损坏 / 越界 / 不匹配直接报错，不重新推理或写入包。仅修改 chart Anchor 并保留同一音频与 analysis 的新包可以复用原 evidence，当前窗口仍显示新包的完整 CID 与源谱面身份

列表按原 canonical frame 排序，`beat` 与 `raw_downbeat` 是独立行，同帧也不合并；每条候选的详情保留本类别内的零起始 `group_index`、原 `original_q`、原 frame、未校准 score 和成员 raw logit。q 是 50 Hz 谱网格坐标，峰组均值可为小数，48 kHz 帧按 `round(q × 960)` 映射并严格保持在 `[0, N)`；raw downbeat 光标保留原帧，nearest / aligned beat 的索引、q 和帧另列为关系，不用对齐帧覆盖原位置

详情先显示未知置信度与只读说明，再展示原证据；`confidence=null` 保持未知，raw logit、sigmoid、strength 和 downbeat_probability 均为未校准分数。选中 raw downbeat 时另标最近 beat 关系，包内分数只是保存的候选聚合，不自动成为 Anchor、音乐真值或已验证置信度

窗口复用现有 final canonical stereo 波形、试听、单一菜单主控、缩放与滚动，不提供采用或编辑 / 导出按钮。源 chart 的作者 Anchor 仍可见，这不是隐藏提示的盲审入口；独立 `workbench-labels` 继续隐藏作者 / 算法提示，候选工作台不把记录写入 Labels 或自动采用标签

本批 Linux x86-64 软件检查通过：reader 8 项窄测、Lab 68 项、修正后的 Clippy / 格式、两轮各 13 语言字形及两版各 467 项冻结输入构建。首版 13 条实际 CLI 为 4 条包校验与 9 条窗口打开前的证据拒绝，不冒充 evidence 正向读取；原生辅助程序另完成四包 reader API 正控，raw / aligned q 在这四包中相同，实际不等坐标只由构造 CPU 例覆盖

首版原生补验为 4 项 API、3 个窗口 / 12 PNG，软件控制与实际 Kira 音源游标推进、暂停 / 停止通过；中文 7 图指定范围目检通过，法语 5 图发现图例换行重叠 Cursor。仅缩短 13 语言图例后重新冻结第二版，实际 1 项 API 与法语 640×480 单窗口 / 5 PNG 经主线程和独立 agent 全图指定范围目检通过；中文、空候选和试听未在第二版重跑，不能把首版三窗口或音频结果改绑为新版全量通过

首轮 std lint、CLI QA 路径错误、辅助程序 E0277 / E0502 和 Unix socket 环境失败、法语图例视觉 FAIL 与定向修复均保留，命令、两版源码 / 二进制与原始结果见[观察记录](../testdata/synthetic/native-beat-workbench-observations-20261008.json)和[原始证据索引](../testdata/synthetic/native-beat-workbench-observations-20261008-raw-index.json)；独立 Labels 逻辑保持，本批未重新进行其原生窗口验证

既有 wholeSpect / 音乐质量 FAIL、未校准 confidence 及 production_admission=false 保留；本入口交付候选复核软件，不扩大 MIR 生产准入、真人盲标、实体手柄、声学试听或设备计时结论

## 40 项导入与证据矩阵

2026-10-08 固定 Linux x86-64 debug Lab 完成十组合成样本 × 源 WAV / 旧 Ogg × 左 / 右声道的 40 次实际导入与 40 次独立 reader API 回读，全部 exit0；覆盖固定 / 非整数 BPM、加速、3/4、6/8、弱起、摇摆、静默、反相及单侧有声。各命令输入与保护文件前后 hash 保持，独占进程组退出；最终回执核对 468 项产品输入和 19 项 native schema 输入仍匹配运行时冻结身份

此矩阵的参考是原合成来源日程，不是新导入最终音频的独立人工真值；每次导入均重新编码，`old_ogg` 分支还包含旧 Ogg 的再次编码。沿用旧 `probe.score` 的 70 ms 闭区间浮点比较与按顺序最早可配对规则，保留有符号误差、未匹配项和原始越界坐标，未平移时间原点或调整质量门槛；三序列的原始累计计数如下

| 候选序列 | TP | FP | FN |
| --- | ---: | ---: | ---: |
| beat | 1912 | 314 | 110 |
| raw downbeat | 488 | 1408 | 100 |
| 包内 aligned downbeat | 492 | 1392 | 96 |

每个序列都有同样 6 项 `both_empty`：四个静默输入和单侧有声样本的两个左声道输入；其空集合匹配不能代表有声音乐检测成功。软件导入与绑定校验通过，旧 wholeSpect / 音乐质量 FAIL、`Candidate / Algorithm`、confidence=None 和 `production_admission=false` 保持，独立真人参考与置信校准继续另验

本矩阵使用冻结 Lab SHA-256 `7cbad66df2ec1a86282e2b5985ebbe4ca0dd27f4b30e5ce10d8bedeb5282dddb`，未应用后续取消或 SDK 初始化修复。40 次会话各保留 51 bytes 的 SDK `.ses` 副产物，固定官方源码静态溯源定位到 ONNX Runtime 1.30.0 的 Microsoft 1DS 会话文件初始化；当前 `with_telemetry(false)` 发生在该初始化之后，不能阻止此工作目录写入。实际网络发送未观测，本矩阵运行时修复与副产物消失验证尚未运行，本批结果不能作为修复 PASS；完整命令、来源日程、计数、副产物和溯源见[矩阵观察](../testdata/synthetic/native-beat-matrix-observations-20261008.json)与[原始证据索引](../testdata/synthetic/native-beat-matrix-observations-20261008-raw-index.json)

## Ctrl+C 取消原生候选导入

```sh
ORT_DISABLE_TELEMETRY=1 /path/to/release/bin/cocobeat-lab import-experimental-beat /path/to/source.wav /path/to/authoring.json left /path/to/new-bundle
```

Linux 直接执行实验导入须在进程启动前设置 `ORT_DISABLE_TELEMETRY=1`，缺少或值不符时在固定 SDK 初始化前拒绝。此保护要求变量在 CreateEnv 前就已设置，SDK 在 CreateEnv 期间读取 flag 并跳过 Microsoft 1DS 本地会话持久化初始化，不把之后的 `.with_telemetry(false)` 当作持久化保护；应用不在运行中修改进程环境。Linux TAR 解包 workflow 已给原生实验 smoke 步骤设置该变量，固定 `8cdda9d` 两架构解包实验导入通过；Windows 命令不要求此 Linux guard，控制台取消另验

Ctrl+C 只向本次原生 Lab CLI 导入提出取消请求，随后等待真实函数返回和正常资源释放。Ready / 曲库 / 工作台没有新增取消或交互 Retry，不更改模型、profile、推理数学、普通手工导入和唯一编码器。取消在源快照、解码 / 重采样、编码 / 最终读回、能量、文件复制 / 校验、前处理每 q、chunk 和 evidence 写入等消费边界检查；同步 I/O、Ogg 预校验和 SDK 环境初始化仍可能等到前后检查点

本次 CLI 独占 SIGINT，使用 `ctrlc::set_handler` 接管后台 shell 继承的 SIG_IGN；安装失败仍返回原错误，同进程真正重复注册仍被库拒绝。固定 `07830f8` 的 Linux 两架构解包 smoke 已实际暴露旧 `try_set_handler` 对继承忽略设置的拒绝，未进入导入；本机相同后台 shell 的旧 / 新 Lab 控制分别复现注册错误和通过注册后在 SDK 初始化前拒绝缺失 telemetry flag，固定 `8cdda9d` 的解包正常导入通过，不替代新的跨平台取消控制

模型装载用 LoadCanceler 发出 best-effort 请求，每个真实 Run 用独立 RunOptions 发出 termination 请求；请求接受或调用返回 Ok 不等于底层已中断，`backend_registered` 只说明句柄已登记，不证明请求时处于 graph / kernel 计算中。真实 ORT / I/O 错误保留；装载若仍返回 Session，则在后续检查点正常释放，Run 若仍返回输出，则先保留实际 raw 输出再检查取消

最终 bundle 发布与取消共用同一门：取消先赢则不执行最终 rename，发布先赢则后续请求记为 late，按 rename 的真实结果返回，不删除已提交目标。注册句柄、Session / outputs 和 encoder 正常释放后才报告最终状态，四对象清理只处理本次拥有的对象，失败 outer staging / evidence 保留，部分目录不作为有效 bundle

stderr 诊断分别记录接受 / late、检查点、backend 请求与真实返回、发布和 finished。`failed.json` 是 attempt 内的失败快照，写入时可能 `finished=false`，wrapper 随后结束 attempt；最终 CLI 诊断或 API 返回后再读句柄的 `finished=true` 才表示实际终态。墙钟 `*_ns` 供本机审计，耗时用同一控制进程的单调时钟另测，不跨进程相减

公开 `NativeBeatCancellation` 句柄仅用于一个 attempt；同进程 API 重试须等上次真实返回，再用新 handle / 新目标串行调用。CLI 没有 Retry：上一次正常退出后重新运行命令并选择新目标；失败目录不用于补造 confidence 或 Ready 状态

本批 Linux x86-64 软件检查为 media 64 项、Lab 72 项、Clippy / 格式和 469 项冻结输入构建通过；首次 E0425 来自旧测试函数名字失去 `super::*` 导入，仅补 `crate::prepare_canonical_audio` 的测试限定后通过，原失败保留。冻结 Lab SHA-256 为 `f6c7c3a8473bd628d80bb6f8dba77ac7f3b40e15d607ad894d0e463b4da33256`，七项真实控制为缺失 flag 拒绝、64 秒正常导入 / 校验、已有目标拒绝、装载请求 / fresh 重试、Run 请求 / fresh 重试及 600 秒 CLI 取消；各进程正常结束，输入与保护文件保持，未用超时 TERM / KILL 收尾

装载与 Run 分别保留 SDK 原 Err `Graph loading canceled due to user request` 和 `Exiting due to terminate flag being set to true`，之后在同一 namespace PID 13 / 17 下用新句柄、新目标重试成功，完整包校验、no-op 四对象与作者 Anchor / Stage 对照通过。600 秒样本先完成模型装载，SIGINT 在完整 native-shape 标记后发出，最终在 `native chunk preparation` 检查点观察到取消、`backend_requests=[]`、零最终发布；它不证明运行图被中断。请求到观测退出为 35527433 ns，包含 5 ms 轮询，是本次单调时钟观察而非信号响应 SLA

缺失 flag 的独占目录内 29 bytes 哨兵保持，六个设置 flag 的独占 CWD 前后均未产生 SDK 会话缓存；旧 40 项矩阵副产物保留，仓库根未知会话文件未读取或清理，实际网络发送未观测。完整原错误、失败快照、同进程重试和源身份见[取消观察](../testdata/synthetic/native-cancellation-observations-20261008.json)与[原始证据索引](../testdata/synthetic/native-cancellation-observations-20261008-raw-index.json)；Windows 控制台、GUI / Ready 取消、graph 占用 / 延迟预算、实体设备 / 真人和音乐质量仍按各自门槛验收，旧 wholeSpect / 音乐质量 FAIL 与 confidence=None 保持

## 同提交四目标发行软件验收

固定 `8cdda9d` 通过 CI 后，Windows / Linux × x86-64 / ARM64 四个原生优化 Game / Lab 包及解包软件检查通过，48 条实际 Lab 命令为 44 条成功和 4 条预期缺失模型拒绝，八份左右声道 tempo 报告共 192000 行完成核验；包内源码、资源、许可台账与字体 QA 排除按该提交身份匹配，见[同提交四目标观察](../testdata/synthetic/release-four-target-8cdda9d-observations-20261008.json)及[原始证据索引](../testdata/synthetic/release-four-target-8cdda9d-observations-20261008-raw-index.json)

原 `07830f8` 四目标 FAIL 单独保留：Windows 两架构因未声明的 `random()` 构建失败，Linux 两架构完成构建后在首次实验导入的 Ctrl+C 注册处失败，tempo 和后续命令未运行，见[原 078 四目标失败观察](../testdata/synthetic/release-four-target-07830f8-observations-20261008.json)及[原始证据索引](../testdata/synthetic/release-four-target-07830f8-observations-20261008-raw-index.json)；更早 `0930d17` 三目标与 `d3b2648` 单 ARM 的历史矩阵保持各自源码范围

Linux ARM64 本次 `--jobs 1` 构建的 GNU time wall 为 13:03.42、最大 RSS 11453596 kbytes，属于单次 runner 环境观察，不作为预算或旧失败根因。此矩阵不验收 Game GUI、干净机器 / Windows VC 前置、实体输入 / 音频、真人音乐参考或实际 tag Release；旧 wholeSpect 19 PASS / 9 FAIL（整体 FAIL）与音乐质量 FAIL 保留，tempo 的 beat_unit / meter / confidence 仍为 None、质量 UNSCORED、production_admission=false
