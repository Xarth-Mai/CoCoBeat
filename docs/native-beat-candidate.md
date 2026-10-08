# 原生 beat / downbeat 实验候选

`cocobeat-lab import-experimental-beat SOURCE_AUDIO AUTHORING_JSON left|right NEW_BUNDLE` 是显式实验入口，沿用源快照、唯一生产编码器、严格最终读回和手工作者内容事务，再从同一最终 canonical 音频生成 beat / downbeat 候选；Ready 菜单及普通手工导入不运行此分析

```sh
/path/to/release/bin/cocobeat-lab import-experimental-beat /path/to/source.wav /path/to/authoring.json left /path/to/new-bundle
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

旧 profile 的 wholeSpect 19 PASS / 9 FAIL（整体 FAIL）及音乐质量 FAIL 全部保留；同输入推理数值一致不证明原生前处理等价官方音乐前处理，也不构成完整 MIR 准入。各目标原生发行接线与实际 tag Release 按[构建发行](build-release.md)逐版本另验；发行 CPU/RSS 预算、十分钟产品取消、同进程 ORT 重试、真实音乐 / 设备 / 真人验收仍为 NOT_RUN

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
