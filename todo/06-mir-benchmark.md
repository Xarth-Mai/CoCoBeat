# 06 · MIR 基准

前置：05 的音频真值稳定。分析库全部隔离在 media 适配器内。

已先完成独立的原始 PCM [onset 基准](../docs/mir-onset-probe.md)：五个可复现样本与十次逐声道检测，软件检查通过；OxiMedia MIR 0.2.1 在固定参数下的候选质量 FAIL，脉冲预测系统性提前，保留原始时间原点和失败结果。当前只覆盖固定/非整数 BPM 脉冲、首尾、静默和反相，不接入生产 MusicAnalysis；最终 Ogg 回读、其余合成类型、真实数据和人工标签继续按下表推进

后续 [时间诊断](../tools/mir-onset-diagnostic/README.md) 已定位窗口坐标与首尾处理，能量差端点适配通过原 10 声道及相位/低幅度/短尾控制，持续音与噪声误报、单样本漏检和恒定电平尾部假峰仍为 FAIL；原始失败和观察清单保持不变

[单边频谱 HFC 候选](../tools/mir-spectral-probe/README.md) 已验证等能量不同频率的真实加权，固定 128 帧 Hamming 窗和 64 帧 hop，按实际 PCM 窗支持区间映射坐标；原 10 声道与 3 个控制通过，另外 5 个控制仍 FAIL，持续音及恒定电平的额外峰降为 0、噪声额外峰降为 1，但这些持续信号的首帧攻击与单样本攻击仍漏检；软件检查和独立审查通过，完整 [观察清单](../testdata/synthetic/mir-spectral-probe/observations-20261003.json) 保留失败，不接入生产 MusicAnalysis

[固定谱通量对照](../tools/mir-flux-probe/README.md) 已保留原 18 项并预先声明 6 个离散音色控制和 1 个连续渐变观察，HFC 原矩阵完全复现，Flux 原矩阵为 11 PASS / 7 FAIL；新增离散控制各 1 PASS / 5 FAIL，等能量换音和静默后起音暴露两者不同能力，持续音额外峰仍阻止准入，见 [观察清单](../testdata/synthetic/mir-flux-probe/observations-20261003.json)

[固定归一化过滤](../tools/mir-flux-gate-probe/README.md) 进一步按预先声明的谱幅值变化比例 0.5 过滤原生 Flux 峰，不移动坐标；新增 6 项控制全部通过，离散额外峰从 537 降为 0，旧 18 项仍 11 PASS / 7 FAIL，包含删除持续音假峰后新增的 2 个首帧漏检；4 项软件测试和独立逐窗复算通过，整体质量仍 FAIL

[近邻、慢起音与叠加声部控制](../tools/mir-flux-gate-probe/README.md#近邻慢起音与叠加声部)已按运行前声明完成：新增 7 项离散控制过滤后 4 PASS / 3 FAIL，额外峰 229 → 0，漏检 2 → 3；2 项慢起音只观察、不评分。384 帧强弱近邻在原生均值选择就漏掉弱峰，弱叠加的实际候选则被全谱归一化过滤删除；旧 25 项全部 44,417 窗重现，累计 31 项离散控制为 21 PASS / 10 FAIL，生产准入仍为 FAIL

[分频带局部变化候选](../tools/mir-flux-gate-probe/README.md#分频带局部变化候选)已实现并完成唯一一次 34 项运行，6 项软件测试、Clippy/构建及独立全量复算通过；离散质量退化为 17 PASS / 14 FAIL，665 个额外峰、6 项旧 PASS 退化。两项近邻恢复，弱叠加实际进入虽恢复仍有 35 个额外峰，修复验收继续 FAIL；独立 f64 FFT 复现全部预测，已定位持续音旁瓣被低幅值频带分母放大

[固定频带分母下限](../tools/mir-flux-gate-probe/README.md#带宽与局部谱峰下限)已完成：7 项软件测试、独立全量复算通过，离散为 21 PASS / 10 FAIL、269 个额外峰和 6 个漏检。两项近邻及弱叠加通过，原 6 项退化恢复 3 项；noise burst、kick、snare 与原 7 项边界仍失败，所有旧结果和新增坐标保留

[固定时间背景门控](../tools/mir-flux-gate-probe/README.md#归一化变化的时间背景门控)已完成唯一一次 34 项运行：8 项软件测试和独立全量复算通过，31 项离散为 20 PASS / 11 FAIL，额外峰 269 → 4、漏检 6 → 16。noise burst 恢复通过，近邻和弱叠加继续通过；两项相位扫描发生新退化，kick / snare 仍有额外峰，目标修复与生产准入继续 FAIL，原基线、全部标签和删峰证据保留

失败诊断表明真 snare 首击的 `E-B = 0.44161` 低于剩余假峰的 `0.51001..0.84773`，继续调整该单一分数下限无法分开两类事件，因此停止这条门控修补链，保留完整矩阵、坐标、Matcher 与所有 FAIL。非零文件首样本不能揭示文件之前的录音历史，旧首帧工程 FAIL 仍保留，不通过硬补首帧、平移预测或只保留干净脉冲取得准入

已准备[原创曲目审阅清单](../testdata/synthetic/dev-song-review/README.md)：现有 64 秒 WAV 的配方枚举 438 个声部起点，合并为 200 条来源候选，另保留 7 个结构边界及原 7 个创作 Anchor；人工 onset / 可玩性全部 pending，不能把触发帧当听觉时间。立体声反相 hat 的下混抵消另作输入边界，来源漂移拒绝、防覆盖和逐字节复现通过；下一步取得独立人工标签再比较新的固定特征路线，外部音乐与编码回读仍单独验收

[Beat This! small0 与 BSD DBN 候选研究](../tools/beat-model-probe/README.md)已完成 Linux x64 CPU 对照：10 项合成源 PCM 与同源真实 Vorbis 编码严格回读各运行 120 行，官方 PyTorch 与 ORT 的 logits 及后处理坐标数值 PASS，编码回读最大 logit 误差为 5.6267e-5；不可变媒体 driver SHA 与 301 项构建输入、模型、源码和原始证据 SHA 均记录在[结果摘要](../tools/beat-model-probe/results-2026-10-07.json)，8 项源关联窄测包含 1 项实际 driver 正向检查与 7 项替换、元数据错误或运行中漂移拒绝

算法质量继续 FAIL，`production_admission=false`：官方 DBN 改善固定节拍和部分 3/4 强拍，变速、6/8 与摇摆仍退步，实验 `[2,3,4]` 未解决这些缺口；编码前后 80/120 行坐标完全相同，其余实际差异与全部越界点保留，未裁剪输出或改变参考拍单位，旧 onset FAIL 基准保持不变

官方前处理拒绝 1 帧与 10 ms 输入，25 ms 可生成谱图；候选含等于 PCM 时长的越界点，后续适配器必须显式报告短输入不足及 `[0, frames)` 越界；beat/downbeat 仅为未校准候选，精确 onset、可靠拍号、TempoRegion、section/repetition 和校准置信度均 Unsupported，不能用空集合代替完整分析能力状态

该批仅交付研究工具与结果摘要，Python 依赖、官方 MIT 模型、BSD 源码副本和完整输出留在 `target/`，未接入产品；madmom 非商用模型未取得或加载，生产原生前处理与 ORT C API、四目标完整归档校验及加载仍 NOT_RUN，真实音乐、人工标签和可玩性验收另行完成

- [x] Beat This! small0 导出与三组后处理窄研究：源 PCM / canonical 回读数值对照及失败质量、短输入、坐标边界和来源证据已保留，研究完成不代表生产准入
- [x] MusicAnalysis v2 数据契约与手工制作接线：analysis 独立 v2，chart / manifest / authoring 仍 v1；固定七项 capability、四状态、三来源、可未知置信度与有界 TempoRegion / repetition 载荷已实现，新手工包为 `canonical-rms-1024-v2`，energy 为实测、sections 为手工，旧 analysis v1 保留未知 capability 与原字节导出身份，见 [包契约](../docs/song-package.md#分析版本与谱面)
- [ ] 完整 MIR 生产算法与质量准入：普通手工导入的默认 capability 保持；显式实验 onset / beat / downbeat / interbeat tempo、结构与重复关系入口已实现未校准 Candidate，默认手工导入的 repetition Unsupported 保持；候选质量准入、置信度校准、自动 Anchor 策略与真人标签继续按各自门槛验收
- [ ] 合成固定/非整数 BPM、变速、3/4、6/8、弱起、静默、切分、摇摆和立体声边界。
- [ ] 原始 PCM 与编码回读分别评估，至少部分帧真值独立手工核对。
- [ ] 明确外部数据集版本、获取方式和许可后再引入适配器。
- [x] 独立人工标签软件入口：完整 SongPackage 来源身份、人工整数帧 / 半开区间、应 / 不应 / 不确定 Anchor、理由与本地审阅别名、另存导入及明确 item_id 双人分歧报告已接 CLI；40 项 Lab 测试、33 项真实 CLI 控制和最终 Clippy 通过，见[独立标签](../docs/independent-labels.md)，构造控制不作为人工真值；新增隐藏作者 / 算法提示的原生 Labels 编辑已有 57 项正式 Lab 测试、Clippy 与冻结构建，已完成分版原生软件补验及最终法语小窗口指定范围目检，旧视觉失败保留；OS IME / 设备及真人盲标另验
- [x] 独立人工音乐事件软件入口：MusicTruth 四条 CLI 消费最终 canonical 音频身份、人工 onset / beat / 每小节第一拍 downbeat、分轨覆盖与双人精确帧分歧；Lab 72 项、最终 4 项窄测 / Clippy / 格式、468 项冻结构建及 20 条真实 CLI 控制通过，见[MusicTruth](../docs/music-truth.md)，构造控制不替代真人参考、拍号 / 节拍单位或完整 MIR 算法准入
- [x] 原生候选与 MusicTruth 覆盖限定的软件对照：显式整数帧容差、同声道 / 音频身份、三流原坐标、半开 connected coverage、计数 / 匹配误差及空覆盖 / 零事件区别已接线；7 项窄测、79 项 Lab、最终 Clippy / 格式、470 输入构建与 16 项实际 CLI（8 成功 / 8 预期拒绝）通过，见[候选对照](../docs/music-truth.md#对照原生候选)，构造参考不关闭真人 / 校准或音乐质量门槛
- [ ] 人工 Anchor 标签包含“不应放 Anchor”；双人标注子集并报告分歧。
- [ ] 分模块测量准确度、置信度可靠性和成本；不合格算法开发期替换。

退出条件：可复现报告解释音乐证据的能力和不足，不把 beat 检测指标直接当作 Anchor 可玩性。

2026-10-07 · 分析依赖边界：用户允许成熟原生 / C / C++ 库，不追求纯 Rust；明确不接受 GPL 组合发行或 Python 分析部署。Aubio、Essentia 与 librosa 不采用所提生产方案，继续核验许可宽松的原生候选，原始研究与质量 FAIL 保持；能力按模块实测准入，未知置信度不自动补谱

2026-10-07 · [AudioFlux MIT 原生候选](../tools/native-onset-check/README.md)完成内置 FFT 构建、原 Matcher 单测、12 项输入边界控制及 29 份原 PCM 的逐 SHA 恢复，全部 34 项质量结果为 2 PASS、28 FAIL、1 项不支持、3 项不评分；原时间坐标、默认峰选择、近邻与首尾失败及未知置信度保持，未接入生产 Python、GPL 或新 codec

首次 sandbox LeakSanitizer 环境失败与后续宿主同二进制 / 同参数完整验证分别保留；宿主无 ASan / UBSan / LSan 诊断，33 份预测与 native 逐字节相同，见[观察记录](../testdata/synthetic/native-onset-observations-20261007.json)。生产准入继续 FAIL，同源 canonical、独立数值 oracle、四目标、长期成本与人工标签另验

- [x] AudioFlux 原生 C 软件研究前置：构建、原输入按 SHA 恢复、原 Matcher 与边界控制、完整原质量矩阵及失败证据
- [x] AudioFlux 短矩阵 sanitizer 验证：保留环境失败，宿主同二进制同参数完整执行，无诊断，质量仍 FAIL
- [x] BTT MIT 原生 tempo 软件研究：固定 48 kHz、native / sanitizer 各 16 项边界控制与原 source / canonical 两声道各 40 条完整记录通过软件检查；原始输出一致，质量未设准入门槛，13/20 编码前后曲线有变化，精确 onset / beat 与完整分析继续未准入，见[研究结果](../tools/native-tempo-check/results-2026-10-07.json)
- [x] 原生 tempo 观察软件入口：`inspect-native-tempo` 从同次完整验证 canonical 音频取显式左 / 右声道，按真实 128 帧 / EOF 保留 raw BPM / period / certainty 与未知音乐语义；BTT 2 / media 67 / Lab 80 / xtask 6、Clippy / 格式 / 边界及 492 输入构建通过，32 秒同包左右各 12000 行原值一致，首轮 QA 整体 FAIL 保留；见[原生 tempo 观察](../docs/native-tempo-candidate.md)，部分尾、平台 / 长曲与音乐质量 / TempoRegion 尚未准入
- [x] 原生谱形变化与相似度诊断软件入口：`inspect-structure-features PACKAGE left|right NEW_REPORT` 复用最终 canonical 音频，真实1024窗 / 24窗 bin、原频带特征 / 相邻变化 / top4邻居和 EOF 支持只读保存；生产 media72 / Lab82、Clippy / 格式 / 边界通过，候选快照另有5 / 2窄测、12条实际 CLI 与600秒45.286秒 / RSS71352 KiB单次 debug 成本，见[原生谱形诊断](../docs/native-structure-features.md)；UNASSESSED / confidence未知且不自动采用，旧 wholeSpect / 音乐质量 FAIL 与新入口四目标原生 NOT_RUN 保持

- [x] 显式原生 beat / downbeat 候选软件入口：最终 canonical 音频、固定 CPU SDK / 模型、MusicAnalysis v2 Candidate / confidence=None、四对象包加原始 evidence 已由 Lab 消费；media 46 / xtask 5、12 条成功命令、四组同实际 spect 数值对照及 13 项事务 / 资源控制通过，普通手工导入四对象保持，见[原生候选](../docs/native-beat-candidate.md)
- [x] 原生 beat / raw downbeat 候选复核消费者：`workbench-beats` 校验 evidence 与包 analysis 绑定，原 q / frame、成员 raw logit、nearest / aligned 关系和未知 confidence 分开显示，共用 canonical stereo 试听；软件检查、首版三窗口软件控制与修正图例后的新版法语单窗口指定视觉补验分别通过，旧视觉 FAIL 保留，见[原生工作台](../docs/native-beat-candidate.md#原生候选工作台)，完整 MIR 算法 / 音乐质量与置信校准仍未准入
- [x] 原生候选合成矩阵软件覆盖：十组合成来源 × 源 WAV / 旧 Ogg × 左 / 右声道完成 40 次实际导入与 40 次 reader API 回读，输入 / 保护文件及冻结身份保持；原日程机械匹配保留误报 / 漏检与 6 项静默空集合，旧 Ogg 再编码不作为最终音频独立真值，原矩阵 SDK 会话副产物及溯源保留，见[矩阵观察](../docs/native-beat-candidate.md#40-项导入与证据矩阵)，音乐质量 FAIL 与真人 / 校准门槛保持
- [x] Linux 原生实验导入取消与 SDK 初始化 guard 软件接线：CLI Ctrl+C、装载 / Run 真实原 Err、同 PID fresh API 重试及 600 秒前处理检查点取消已验；media 64 / Lab 72、Clippy / 格式、469 输入构建和七项真实控制通过，零取消发布与 owned-CWD 会话文件边界保留，见[取消软件记录](../docs/native-beat-candidate.md#ctrlc-取消原生候选导入)，不扩大 Ready / GUI / Windows、运行图延迟预算或音乐质量结论

旧前处理 wholeSpect 19 PASS / 9 FAIL（整体 FAIL）和音乐质量 FAIL 保留，本批同输入 logits / q 一致不构成前处理等价或生产 MIR 准入；独立真人标签、置信校准、同 ref 完整四目标与发行 CPU/RSS 预算仍未完成；600 秒 debug 成本已有独立观察，Linux 实验导入取消按新软件证据限定

2026-10-08 · 独立标签增加 [`adopt-labeled-anchors`](../docs/independent-labels.md#明确采用为-anchor) 明确采用 CLI，Source CID 与标签原字节 hash 绑定指定肯定点，保存后 hash 由 `import-labels` receipt 提供；[软件验证已通过](../docs/independent-labels.md#明确采用为-anchor)。此接口消费人工记录，不产出自动 onset、置信度或策略校准，完整 MIR 与真实双人标签退出保持未完成

- [x] 显式自动分析软件候选：`import-experimental-analysis` 增加真实 HFC onset 与原相邻 beat 区间 tempo，`compile-structure-candidate` 明确生成 sections / cues 新包；media83 / Lab85、Clippy / 格式 / 边界 / 构建、原 PCM32与编码回读32项机制控制及实际 SDK / reader 消费通过，见[自动导入](../docs/native-beat-candidate.md#显式自动分析候选导入)与[结构候选](../docs/native-structure-features.md#显式编译结构候选包)，confidence未知、不自动 Anchor，音乐质量与真人参考未准入

- [x] 自动 HFC onset 的 MusicTruth 对照消费者软件验收：固定自动 profile 报告 v2，原帧 / strength / 独立 records 按显式容差与 reviewed 覆盖对照，Candidate 空流保留漏检，Unsupported 保留原因与空 comparison；旧 beat-only 报告 v1 保持，Lab92项与必要检查、4条实际CLI及600秒七资源reader通过，旧v1完整字节保持；音乐质量和置信校准另验，见[候选对照](../docs/music-truth.md#对照原生候选)

- [x] 原生重复关系软件候选：`compile-repetition-candidate` 固定网格 / 原速原调、完整非零bin、至多64个种子lag及64条非重叠关系，Candidate / Algorithm / None，原音频 / chart / sections / cues / Anchor保持；生产media85 / Lab85、Clippy / 格式 / 边界 / Lab构建已通过，原PCM34 / canonical34项预登记机制控制已PASS，600秒同源repetition实际发布18关系并通过typed守恒，wall97.871086秒 / kernel RSS72360 KiB；结构600秒capErr保留，成功出版成本另验，见[重复候选契约](../docs/native-structure-features.md#显式编译重复关系候选包)，完整MIR / 音乐语义 / 校准退出保持未完成

- [x] 重复关系工作台消费者：同一结构模式展示两侧原端点与双区间、区分origin / inspection声道，显式Seek保持；Lab92项、两份生产回读、大小两窗及6PNG指定目检通过，首QA期望FAIL保留，见[消费者观察](../testdata/synthetic/candidate-consumers-observations-20261008.json)，音乐质量与当前来源四目标另验
