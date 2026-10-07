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

旧 profile 的 wholeSpect 19 PASS / 9 FAIL（整体 FAIL）及音乐质量 FAIL 全部保留；同输入推理数值一致不证明原生前处理等价官方音乐前处理，也不构成完整 MIR 准入。四目标 workflow 已准备实际解包后的候选 / 手工入口检查，但这轮四目标原生 GitHub Actions 和新 tag Release 未运行；十分钟成本 / 取消、同进程 ORT 重试、真实音乐 / 设备 / 真人验收仍为 NOT_RUN
