# 独立音乐事件人工记录

MusicTruth v1 保存最终 canonical 音频上人工审阅的 onset、beat 和 downbeat，独立于 Anchor 的应 / 不应 / 不确定标签；提供来源、空模板、人工 JSON 导入、双人覆盖限定的精确帧对照，以及显式容差的原生候选机械对照，不改变 SongPackage、MusicAnalysis、Chart、Replay 或 Anchor

四条人工记录命令已接正式 Lab，软件机制验证通过；原生候选对照另按下文验证范围记录；真人听审、独立真值、音乐质量和算法准入继续另验

## 四条命令

```sh
cargo run --locked -p cocobeat-lab -- music-truth-source /path/to/package
cargo run --locked -p cocobeat-lab -- music-truth-template /path/to/package reviewer-a stereo /path/to/a.empty.json
cargo run --locked -p cocobeat-lab -- import-music-truth /path/to/package /path/to/a.manual.json /path/to/a.validated.json
cargo run --locked -p cocobeat-lab -- compare-music-truth /path/to/package /path/to/a.validated.json /path/to/b.validated.json /path/to/new-comparison.json
```

`music-truth-source` 完整验证四对象后向 stdout 输出音频来源；`music-truth-template` 创建三个空 tracks，`channel` 仅接受 `stereo | left | right`，stereo 表示原立体声播放而非下混，reviewer 是无首尾空白的 1..64 UTF-8 字节本地别名。审阅者在包外用普通 JSON 编辑器填写人工精确帧和覆盖声明，再用 `import-music-truth` 有界校验并另存新文件；没有专用 MusicTruth GUI，也不从工作台点击时刻、作者触发帧、模型峰或游戏输入自动生成记录

三个必填 tracks 为 onset、beat、downbeat，每轨含 `reviewed: [{start_frame, end_frame}]` 和 `frames: [integer_frame]`；onset 是审阅者听辨的起音位置，beat 是按审阅者明确节拍单位标出的拍点，downbeat 是每小节的第一拍，不是任意强拍。各轨分别审阅、分别比较，不自动复制 downbeat 为 beat，也不强制把一轨事件补入另一轨

v1 不保存拍号或节拍单位字段，不默认 quarter、eighth 或其他拍单位；实际人工参考用于算法质量评估前，必须在审阅过程记录中明确对应区间的拍号 / 节拍单位与解释，变更或未知部分也须明确，不能从整数帧数组、文件名、重音或模型分数推断。当前四条 CLI 的机械对照不验证这些音乐语义，不给 F-score 或算法准入

## 来源与保存

适用音频身份为 `audio_blake3 + canonical_frames + canonical_sample_rate=48000 + channels=2 + audio_basis=canonical_decoded`；audio hash 是完整已验证的 `song.audio.ogg` 原字节 BLAKE3，整数 sample frame 从最终 canonical 第 0 帧计算，双声道共用同一时间轴，不接受秒 / 毫秒、小数、offset 或设备时钟

`origin_content_id` 保留文档最初声明的完整包 CID；chart 或 analysis 改变但上述音频身份完全相同时可复用，import 不改写 origin，receipt / report 用 `validated_content_id` 另记本次实际验证的包。历史 origin 只是文档声明，不表示本次重新验证了历史包；重新编码或帧数 / rate / channels / basis 改变拒绝复用，不平移或迁移人工帧

保存前重新验证本次目标包的完整 CID，操作中 chart / analysis 漂移仍拒绝。输出须是包外的新文件，父目录已存在，已有文件 / 目录 / symlink 拒绝覆盖；输入须为有界普通文件，hash 与 parse 消费同一次读取。import receipt 的 `raw_input_blake3` 与实际另存字节的 `saved_blake3` 分开，空白变化可能使两者不同，写入成功后才报告 receipt

## 覆盖与精确分歧

每类 `reviewed` 按序保存不重叠的半开区间 `[start_frame, end_frame)`，范围为 `0 <= start < end <= N`；相邻区间可保持原样。每类 `frames` 为严格递增的整数数组，范围为 `0 <= frame < N`，每个事件必须处在本类声明覆盖内；不排序、去重、裁切、补点或纠正输入，同帧不同事件种类允许

覆盖是审阅者声明已对该类事件完成区间内的穷尽查找；空 coverage 加空事件表示未审阅，非空 coverage 加空事件表示声明该区间没有这类事件，只标几个听到的点不能当作整曲已覆盖。JSON 校验只检查声明自洽，不能证明真人听过、覆盖真实、独立盲审或音乐判断正确

`compare-music-truth` 要求相同音频身份 / basis / channel 和不同 reviewer 别名，按各类双方 `reviewed` 的交集输出 `mutual_reviewed`、`same_frames`、`left_only_frames`、`right_only_frames`、`left_uncompared_frames`、`right_uncompared_frames`。只有交集内完全同帧才记 same，一侧独有只是人工分歧，不是模型 FP / FN 或已裁定漏标；交集外事件保持 uncompared，空交集表示无可比覆盖，不给 100% 一致或准确率

每份文档最多 1 MiB，三轨合计最多 65,536 个事件和 1,024 个覆盖区间，比较报告最多 4 MiB；version / 字段 / basis / channel 严格，额外 confidence、NaN / Infinity、小数帧、重复 / 越界 / 未覆盖事件拒绝。实际序列化输出超过字节上限也拒绝，不截断；双人精确帧对照中，整数 100 与 101 是明确分歧，不提供容差、相位校正、自动共识、F-score；原生候选机械对照按下文显式容差另报

真实审阅须另保存原音频包、原 JSON、对应二进制 / 来源 receipt、播放视角及实际审阅记录；既有候选质量 FAIL、真人标注 / 校准与完整 MIR 退出继续按各自门槛验收，不由本批机械对照替代

## 对照原生候选

```sh
cargo run --locked -p cocobeat-lab -- compare-native-beats /path/to/package /path/to/evidence /path/to/reviewer.left.json 0 /path/to/new-native-report.json
```

`compare-native-beats PACKAGE EVIDENCE MUSIC_TRUTH_JSON TOLERANCE_FRAMES NEW_REPORT` 读取经过包绑定校验的已有原生 evidence 与独立人工文档，显式容差为 `0..=N` 的整数 canonical 帧，没有默认值。原生 channel 0 只接受人工 left、channel 1 只接受 right，stereo / 相反声道拒绝，不下混；音频 BLAKE3、N、48 kHz、双声道和 `canonical_decoded` basis 必须相同。报告分别保留本次完整 `validated_content_id` 与人工文档历史 `origin_content_id`，后者不是重新验证历史包的证明

报告把 `raw_beat` 与人工 beat、`raw_downbeat` 与人工 downbeat、`package_aligned` 与人工 downbeat 分成三流，不相加或用对齐结果替换原坐标。`records` 保留 kind / group_index / original_q / frame / 未校准 score / nearest alignment，三流按 record_index 引用；package aligned 只取有 package downbeat 分数的 beat，每个 beat 一次，不把多个 raw group 展开成多个包内事件

每类只在人工声明的半开 reviewed 区间内匹配，相邻且无间隙的区间可合并为 connected matching_components；有间隙则独立处理，误差即使在容差内也不跨未审阅区间。保持原顺序，以最早可配对的一对一匹配计数，绝对误差 `<= TOLERANCE_FRAMES`，不另做最近误差、相位校正或参考事件移动

每流分别记录覆盖帧数、候选总数 / 区间内 / 区间外、匹配与未匹配计数、真实原坐标及 candidate-minus-truth 有符号帧误差；区间外候选仅是 uncompared，不作为误报。匹配误差的绝对值中位数为偶数中间两值平均，P95 使用 nearest-rank，max 保持整数帧，空匹配分布为 null。没有 reviewed 区间时状态为 `NO_COMPARABLE_COVERAGE`；已审阅但零事件仍为机械对照，不等于未审阅，也不给 perfect / F1 或总体准确率

当前 MusicTruth v1 没有拍号 / 节拍单位字段，报告明确保留 `beat_unit=null`、`meter=null`、`reference_semantics=unrecorded_in_music_truth_v1`，不能从机械帧对照推出音乐语义。confidence=None、quality_status=UNASSESSED、production_admission=false、旧音乐 / wholeSpect FAIL 保持；onset 没有此候选流，不自动形成 Anchor 或生产算法准入

输出为包外的新普通 JSON，父目录须存在，已有文件 / 目录 / symlink 拒绝覆盖，最多 4 MiB，超限报错而非截断；保存前重新完整验证当前包，报告绑定本次读到的人工原字节 BLAKE3 和原生 summary 身份，不声称之后证据文件不会变化

新增 7 项检查与 Lab 全量 79 项通过，仅移除测试比较的冗余借用并修正格式后，最终 7 项窄测、Clippy / 格式通过，原失败保留；470 项冻结输入构建通过。冻结 Lab SHA-256 为 `8df22646077d3f76c335692be31d8c43d4f14ec8987fd00c852bab164a0ab88e`，16 项实际 CLI 为 8 项成功、8 项预期拒绝，覆盖左右声道、容差边界、不跨 gap、未审阅 / 已审阅零事件及身份 / 输出拒绝；各进程正常退出，输入、保护文件与原包 / evidence 保持

本批复用旧实际 evidence 并显式构造软件 oracle，CLI 不重新运行 SDK / 模型、编码器或 GUI；这些帧不是人工音乐真值。原始结果与构造参考见[对照观察](../testdata/synthetic/native-beat-compare-observations-20261008.json)及[原始证据索引](../testdata/synthetic/native-beat-compare-observations-20261008-raw-index.json)，拍号 / 节拍单位参考、真人质量、置信校准和算法准入仍未完成

## 软件验证

2026-10-08 的 Lab 全量 72 项通过，其中包含 4 项 MusicTruth 检查；首次格式检查和测试表达式的 `int_plus_one` Clippy 失败保留，仅等价改写该测试与签名格式后，最终 4 项窄测、Clippy / 格式通过。468 项冻结输入构建前后一致，固定 Lab 的 20 条实际 CLI 为 9 条成功、11 条预期拒绝，完整小样本覆盖 oracle、原输入 / 保存后 hash 关系、同音频跨 chart CID 复用、未审阅与已审阅零事件区别、新输出防覆盖及源包 / 输入字节保持通过

这些输入是构造的软件控制，不是人类音乐标注；精确范围与历史失败见[观察记录](../testdata/synthetic/music-truth-observations-20261008.json)及[原始证据索引](../testdata/synthetic/music-truth-observations-20261008-raw-index.json)。本批没有新增 GUI / 试听 / 模型运行、symlink CLI 实跑、写入故障 / 超时注入或完整并发来源替换验收，也未独立外部重算 BLAKE3；跨平台、真人盲审、拍号 / 节拍单位参考、校准及算法音乐质量保持 NOT RUN
