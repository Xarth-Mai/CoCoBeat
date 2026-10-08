# 原生谱形变化与相似度诊断

`cocobeat-lab inspect-structure-features PACKAGE left|right NEW_REPORT` 只读输出最终 canonical 音频的粗谱形、相邻变化及相似邻居，供作者定位需要听审的位置；歌曲段落 / 重复关系、置信校准和自动舞台编排仍按独立音乐证据验收

```sh
cargo run --locked -p cocobeat-lab -- inspect-structure-features /path/to/package left /path/to/new-structure.json
cargo run --locked -p cocobeat-lab -- inspect-structure-features /path/to/package right /path/to/new-structure-right.json
```

输出父目录须已存在，报告须位于原包之外且尚不存在；复用完整四对象校验、同次 owned 音频快照、写出前 Source 回读和有界 create_new 保存，4 MiB 超限拒绝，已有输出保持。报告绑定 CID / audio BLAKE3 / canonical N / decoded basis，保存原 profile、48 kHz 双声道及显式左 / 右声道；本命令不修改包、Authoring、Anchor、SectionCue、Stage、Ready 或 Replay

## 固定特征与坐标

profile 为 `canonical-spectrum-1024x24-v1-candidate`，直接消费选中声道的真实样本，不下混、归一化、reflect 或 EOF 补零；复用已安装的 OxiMedia 安全 FFT，不调用 beat 模型或 SDK，不新增 GPL 或 Python 产品分析后端

1024 帧不重叠 Rectangle 窗按原 forward magnitude 取单边 bin `0..=512`，八个频带边界为 `[0,4,8,16,32,64,128,256,513]`；每窗先求各带 magnitude-squared 总和，再以真实完整窗数求均值并保存 `log1p`，不倍增单边功率。这些是固定规则的谱描述符，不是物理声压或音乐类别

每 24 窗构成一个 24576-frame bin，即 0.512 秒，区间为 `[i*24576,min((i+1)*24576,N))`。所有真实样本参与 f64 RMS / peak；不足1024帧的尾部只参与能量，另保留 `spectral_frames` / `partial_tail_frames`。没有完整窗时 `INSUFFICIENT_FULL_WINDOW`、谱字段为 null；零范数或缺谱的相似度不可用，不生成静默匹配

非零八维 log-power 描述符输出原始 cosine，相邻变化为 `1 - raw_cosine`；各 bin 从其他有效 bin 中保留至多四个最高分邻居，按 score 降序、原 index 升序稳定排序并排除自己。邻居可以相邻，分数不裁剪、不设音乐阈值，不拼接重复区间；整数 canonical 支持与原值保留，bin 端点不改称歌曲结构边界

报告 `confidence` / `beat_unit` / `meter` 为 null，`quality_status=UNASSESSED`、`production_admission=false`。`sections_context` 仅记录原 analysis sections 数量和原 capability；legacy 缺省保持 null，已有作者元数据不作为新描述符的音乐真值。只读 JSON 和原生审阅工作台共用这些原值，自动采用另行验收

## 只读结构工作台

`workbench-candidates PACKAGE --structure left|right [--locale CODE]` 用显式声道打开谱形诊断，复用完整包校验与同次 owned canonical PCM 读取，不要求预先生成报告。`--locale` 复用游戏支持的 13 个语言变体、Noto Sans 和现有主控 / 焦点规则

```sh
cargo run --locked -p cocobeat-lab -- workbench-candidates /path/to/package --structure left --locale zh-CN
cargo run --locked -p cocobeat-lab -- workbench-candidates /path/to/package --structure right --locale fr
```

列表保留原 bin 编号与完整 `[start,end)` 整数区间；时间线蓝色表示选中区间、紫色表示至多四个相似邻居、白色表示作者 Anchor / SectionCue，上下文来自现有谱面，不是盲标。详情保留原 JSON 的 RMS / peak、频带值、相似度、尾部状态、Source 与未知字段；相似区间不命名为音乐段落，也不自动生成 Anchor

点击按原半开区间选中，真实最后不足窗的尾部仍可浏览；EOF 不映射到任何 bin，光标到达 N 时保留原选中记录。编辑、拖动、撤销 / 重做和导出快捷键沿用只读门控，源包及原历史保持。试听复用实际 Kira 生命周期，明确播放才创建输出，分析的 left / right 选择不改变原立体声播放

本轮生产 media 73 项 / Lab 85 项与 i18n 3 项测试、两包全目标 Clippy、格式、workspace 边界和 Lab 构建通过。9 条实际生产 CLI 控制为 7 次预期拒绝及左右两次旧诊断回归；左右报告与原快照字节相同、原包四对象保持，这些命令不计为原生窗口正向验收

两个原生窗口通过：中文 1280×800 的125区间浏览、详情滚动、软件键盘＋双手柄主控 / 失焦门控与实际 Kira 播放 / 暂停 / 停止，法语 640×480 的两区间、真实一帧尾及 EOF；六张实际 PNG 均经主线程与独立审查逐张目检。原包、谱面与372项构建输入保持，原始分析值与旧报告的 binary64 数值精确一致，见[工作台观察](../testdata/synthetic/structure-workbench-observations-20261008.json)

首轮两个窗口 FAIL 保留：未启用 float_roundtrip 的 QA JSON 解析产生462 / 3处一 ULP 读回差异；仅修正 target 验收脚本，统一解析路径并另核全部原始 binary64 叶值，生产算法、原报告和容差未改。软件 callback / 注入输入不证明声学输出、物理手柄、鼠标一帧精度或真人音乐参考，十分钟 GUI 成本与新功能四目标原生构建仍未验

## 显式编译结构候选包

`compile-structure-candidate PACKAGE left|right NEW_PACKAGE` 从已验证的 analysis v2 包生成真实候选 sections 和对应 SectionCue，输出到源包外的全新目录

```sh
cargo run --locked -p cocobeat-lab -- compile-structure-candidate /path/to/package left /path/to/new-structure-package
```

固定 profile 为 `canonical-logbands-4x4-novelty-v1-candidate`：复用原1024×24谱形 bin，边界两侧各聚合4个完整非零 bin，以 `0.5*(1-cosine)+0.5*min(abs(log(RMS_right/RMS_left))/log(4),1)` 求 novelty。分数至少0.35，原位置为 bin 端点；两侧各自最大 / 最小 RMS 比不超过2、每个 bin 与本侧均值描述符距离不超过0.15，检查前后2位置的局部峰。边界离首尾及彼此至少8个 bin（4.096秒），最多64个支持边界；不足完整上下文、没有支持边界或超限均返回错误，不用作者旧段落或全曲单段作 fallback

成功包的 sections 覆盖 `[0,N)`，以 `structure-candidate-0000` 等机械标签保存，cues 位于各候选段落起点；sections capability 为 `Candidate / Algorithm / confidence=None`，不推断 Verse / Chorus、重复关系或音乐语义。原 analysis sections / chart cues 被明确替换，最终音频、N、作者 Anchor、ruleset 和其他分析事实保持；发布前重新验证原 Source，原包与已有目标保持

新包的 analysis / chart 和完整 CID 改变，diagnostics 记录来源旧 CID、原 sections/cues 数量及原 capability。旧 beat evidence 仍属于原 analysis / CID，不能改写为新结构包的证据；需要分别保留来源身份和本次输出身份

本轮预先声明的14组 / 16份原创 WAV 控制按左右声道完成原 PCM 32项和真实编码回读32项，64项机制检查通过，包括预期无支持 / 短输入 / 超限拒绝；这是合成配方 oracle，编码回读容差为24576帧，不是真人段落或音质真值。生产 media83 / Lab85、两包全目标 Clippy、格式、边界和 Lab 构建通过，实际左右候选新包均已生成，左包另经验证及 Stage CLI 消费，见[自动分析软件观察](../testdata/synthetic/automatic-analysis-observations-20261008.json)

上述 CLI 批次当时的新 Game / UI、600秒成本及四目标原生运行尚未验收；旧 wholeSpect / 音乐质量 FAIL、未知置信度和独立真人参考边界保持

## 显式编译重复关系候选包

`compile-repetition-candidate PACKAGE left|right NEW_PACKAGE` 从完整验证的 analysis v2 包生成谱形重复区间，输出父目录须已存在，目标须为源包之外的新目录

```sh
cargo run --locked -p cocobeat-lab -- compile-repetition-candidate /path/to/package left /path/to/new-repetition-package
```

固定 profile 为 `canonical-logbands-diagonal-8bin-v1-candidate`，复用原1024×24谱形 bin；只读描述符及其 top4 邻居仍不拼接区间，新的独立编译入口沿这些邻居产生种子。只取目标在后、lag至少8个 bin、cosine至少0.95的完整非零 bin 邻居，按种子数量降序、最大种子cosine降序、lag升序选择至多64个 lag，搜索固定网格、原速原调，不穷举所有重复关系

每个 lag 上连续对齐的两段至少各8个完整 bin（4.096秒），逐对 cosine至少0.95、较大 / 较小 RMS 比不超过2；至少两处相同相邻位置在两侧同时满足 `1-cosine>=0.05`，避免恒定音色形成重复关系。两段互不重叠，只采用完整24576帧 bin，不把真实短尾补齐；关系按原source / target帧排序去重，最多64条，第65条支持关系返回错误，输入不足或没有支持关系也返回错误，不用原重复记录或默认关系作 fallback

成功包保存原半开 source / target 四端点，repetition capability 为 `Candidate / Algorithm / confidence=None`、各关系confidence为None，支持分数保留于diagnostics而非校准置信度。仅 repetition capability / payload、analysis profile / diagnostics改变，原音频与N、整份chart、sections、SectionCue、Anchor及其他分析事实保持；发布前再次完整核对原Source，diagnostics绑定来源旧CID、音频hash及原repetition状态，新analysis使完整CID改变

同一 `workbench-candidates PACKAGE --structure left|right` 现可审阅已编译 repetition：原谱形 bin 列表后，每条原关系追加 `repetition[index].source` / `.target` 两行，详情保留原 source_start / source_end / target_start / target_end。选择任一行同时标出两个原半开区间，独立于原 bin / 相似邻居条带；选择只把工作台光标定位至该侧起点，明确使用 Seek 才跳转试听，原立体声播放、源包和 chart 保持

header 的 `inspection_channel` 是本次谱形诊断声道，`origin_channel` 仅在编译 profile、Algorithm 来源及 diagnostics 的音频 hash / N 相符时显示原记录声道，不用本次 left / right 改写来源。未知 profile、损坏 diagnostics 或 Authored / legacy 元数据保留未知来源，原 capability / confidence 和 diagnostics 另列；1280×800中文 / 640×480法语两窗口软件行为及6张PNG指定目检通过，原鼠标QA期望FAIL与仅一行预期修复保留，见[消费者观察](../testdata/synthetic/candidate-consumers-observations-20261008.json)

重复候选编译批次的生产 media85 / Lab85、两包全目标Clippy、格式、workspace边界与Lab构建均通过。17份原创WAV的原PCM34项与真实编码回读34项预登记控制均通过，各为13项正向exit0 / 21项预期拒绝exit1、TP17 / FP0 / FN0；原PCM四端点容差0、canonical四端点各24576帧，精确关系数量与原oracle保持。canonical批次121条实际命令为93次exit0 / 28次exit1，另10项目标保护 / 确定性 / 旧结构四对象回归 / 作者记录非fallback控制通过，source373、输入、旧包及两冻结二进制保持，全部owned进程组退出，见[重复候选观察](../testdata/synthetic/repetition-candidate-observations-20261008.json)

首次Clippy的构造测试范围循环FAIL与等价迭代器修复保留。68项控制是原创配方机制oracle，不证明音乐重复语义、变速 / 转调、置信校准或自动舞台编排；600秒成本单独取证，完整MIR与旧wholeSpect / 音乐质量FAIL保持，沿用现有原生依赖，不新增GPL或产品Python分析运行时

600秒 / N28800000同源成本现已独立观察：自动analysis实际成功发布并完整校验，wall310.879648秒 / 单child kernel RSS1593272 KiB；结构编译在62.510735秒 / 65820 KiB返回超过64个支持边界的原Err，零发布；repetition在97.871086秒 / 72360 KiB成功发布18条Candidate关系，独立strict验证与typed内容守恒通过，原audio / N / chart / sections保持。所有源码和输入hash保持、进程组退出，预声明wall / RSS操作预算未触发；后验耗时另列，kernel与/proc采样峰值分别保存。该Linux x86-64 debug单样本不是发行性能或音乐质量准入，结构600秒成功出版成本和600秒SDK证据reader对照仍NOT_RUN，完整收据见[重复候选观察](../testdata/synthetic/repetition-candidate-observations-20261008.json)

## 软件观察与成本

完整 499 文件候选快照完成 media 5 项 / Lab 2 项窄测、两包全目标 Clippy、格式及 debug Lab 构建；499 由 492 项生产基底、3 项测试 fixture、2 项已有 example 和 2 个新增模块组成。首次 497 文件快照遗漏两个已声明的 native Vorbis example，Clippy exit101 保留；只补源闭包后通过，候选算法不变。以下 CLI 结果绑定该快照冻结 Lab，不能改绑为后来生产二进制的实际运行

此前 JSON 入口接线（`a612ee1`）的四个 Rust 文件与候选逐字节一致；该批生产 media 72 项 / Lab 82 项测试、两包全目标 Clippy、格式、workspace 边界、Lab 构建及一次实际 short-left CLI 均通过，六条命令均 exit0。该批生产 Lab 的 short-left 报告与候选快照逐字节相同、原四对象保持，见[生产检查记录](../testdata/synthetic/structure-feature-production-checks-20261008.json)；12条完整 CLI 仍绑定原快照，新命令四目标原生验证仍为 NOT_RUN

12 条实际 CLI 为 2 次手工导入、6 次正向诊断和 4 次预期拒绝，分别为 8 次 exit0 / 4 次 exit1；覆盖显式左右、既有32秒 / 64秒包、真实短尾、600秒非静音包、非法声道、已有输出、包内输出和损坏音频身份。各命令预先限定180秒、无超时，原包 / 输入及冻结身份保持，命令与原始报告见[观察记录](../testdata/synthetic/structure-feature-observations-20261008.json)及[原始证据索引](../testdata/synthetic/structure-feature-observations-20261008-raw-index.json)

真实短尾 N=24577 的左右报告各保留 `[24576,24577)` 最后1帧非零 RMS / peak，谱为 null、邻居为空、相邻相似度不可用；600秒左声道 N=28800000 产生1172个 bin / 28125个完整窗，本机 wall 45.286秒、单子进程峰值 RSS 71352 KiB。成本来自一次 Linux x86-64 debug 快照进程，包含包读取 / 特征 / 保存路径，不是发行预算、跨硬件保证或新增特征常驻内存上限

报告的有限性、原半开区间、尾部支持、log1p 及全部有效候选的 top4 以预先声明的数值容差复核；CLI 保存 BLAKE3 来自实际 stdout，独立报告字节核验使用 SHA256，未冒充独立 BLAKE3 重算。旧64秒来源总报告的 GPU 不可用 FAIL 与首轮快照失败保留；本批 CPU 诊断不改写旧视觉结果

旧 wholeSpect 19 PASS / 9 FAIL（整体 FAIL）与音乐质量 FAIL 保留，描述符机制检查不关闭完整 MIR、可靠 section / repetition、Anchor 校准或舞台自动编排。新入口四目标原生构建、发行 CPU/RSS 预算、窗口 / 声学 / 设备及独立真人结构参考继续按各自证据验收，既有 `8cdda9d` 四目标结果只覆盖其原功能版本

## 自动候选包的实际游戏接线

2026-10-08，两首真实生成候选包分别以786432 / 1474560帧完成原生窗口软件验收：完整开场后保持Ready至少1秒，先以真实 KeyboardInput 接管菜单主控，再用独立新确认启动；实际 Kira source cursor 推进，原 cue ID / 帧切换、EOF Finished及关闭worker释放通过。短曲经过Plaza，长曲经过Plaza / Curve / Bridge；只读实际9张地面Mesh的514个有限顶点，两个阶段的position指纹改变，与Stage整数采样分别记录

新增 dormant `automatic-structure` 观察场景复用现 library observer，默认游戏不安装；旧场景保持原Update顺序，新场景在scene / UI更新后采样，不写Game / Session / 输入状态。runtime174项、Clippy、Game构建、格式及边界检查通过，6张实际1280×800 zh-CN PNG分别目检，Ready图只展示场景 / HUD，不作为菜单面板证明，见[原生接线观察](../testdata/synthetic/automatic-structure-native-observations-20261008.json)

首次Clippy tuple类型复杂度拒绝与两次85秒Ready窗口FAIL保留：最初QA少了主控接管后的第二次新确认，game / wrapper exit0不能覆盖失败；只补QA输入顺序及类型别名后，冻结新源码 / Game重新实际运行通过，生产输入规则、候选算法和包均未改。实际Mesh读回和PNG不作为逐顶点理论证明，原生日志的ICU4X Chinese/Japanese分段数据警告保留；音乐质量、物理输入 / 音频、600秒成本和新四目标验收继续独立进行

2026-10-08 · 工作台消费者补验：Lab92项、Clippy、格式、边界与构建通过；新版Lab对两份既有重复包的实际谱形回读均exit0，大小两窗经真实输入选择每条关系两侧、显式Seek、Kira源游标推进 / Pause / Stop、详情到底与原bin半开点击 / EOF通过，源码373及源包 / 基准 / 二进制保持，进程组全部释放。相邻source / target会形成连续同色条带，四端点以原详情和行区间保留；原首窗FAIL是QA将点击帧错当选bin后的游标，只修冻结QA的一行预期，生产逻辑保持，声学 / 物理输入 / 音乐质量和当前来源四目标另验
