# 音乐、动作与四世界表现

本轮以 60–90 秒有声可玩样板建立音乐、角色、场景与共同反馈的一致表现，再扩展四个完整世界；每个里程碑交付带声音的实机片段、可玩构建与对应提交，软件检查、画面目检、听感与设备体验分别记录

## 共同约定

离线导入提取局部音级分布、调性与和弦候选、密度、能量和亮度，保留算法版本、来源与未校准可信程度；`MusicAnalysis` v3 在既有四对象 SongPackage 内保存可选 presentation，旧 v1/v2 包保持可加载，运行时查询预计算结果

音乐表现使用同一 SongTime 查询，内容身份与事件身份决定变体，暂停、跳转与 Replay 重建展示；本地 Hit 即刻反馈，共同音效与双人动作只消费已确认 Free Sync / Anchor Sync，音乐表现不改变 Anchor、输入映射或判定规则

每首歌自动推荐世界、音色与动作风格，Ready 提供预览、手动覆盖与恢复自动，选择按歌曲保存；单曲以一个世界为主，段落改变地标、构图和动态事件，重复乐句保留动机而不机械复制店面

## 声音与动作

即时打击层提供触感，可靠和声才叠加音高；六类音色为木质打击、清脆打击、鼓与拍手、拨弦、玻璃铃音、弹性电子，每类具有力度层和避免连续重复的变体

P1/P2 使用可辨音色与声像，双人确认触发互补和声、琶音或短收束句；音高遵循当前和弦与平滑声部连接，低可信、噪声与纯打击段使用无音高反馈，原曲音高保持原值

密集音乐缩短反馈尾音与调整混音占用，保留音乐和反馈独立音量、复音上限与防削波；素材选用精简 VCSL CC0 子集与原创合成声音，每个导出文件记录源地址、许可、处理和哈希

角色基础动作包括点按、踏步、小跳、侧跳、半转、蓄力回弹、招手与恢复，每段具有准备、接触和余势；双人动作包括碰肩、击掌、交叉跃步、镜像旋转、共同跃起、牵引收束，强动作可被新输入、暂停与转场正确中断或融合

玩家标签跟随角色位置，轨道与 Anchor 提示始终可读；共同成功形成局部接触、同伴回应、汇聚和高潮四级反馈，持续合作增加层次，单次成功保持变化

## 四个世界

| 世界 | 三段空间 | 音色与动作 |
|---|---|---|
| 霓虹城市 | 雨夜市集、空中轨道、屋顶舞台 | 电子与打击，利落踏步和旋转 |
| 发光森林 | 萤光林地、湿润溪流、巨树冠层 | 木质与拨弦，轻跳、摆动与呼应 |
| 糖果乐园 | 玩具街道、旋转游乐装置、气球庆典 | 清脆、铃音与弹性声音，夸张弹跳 |
| 星海剧场 | 水晶峡谷、浮空岛、星环舞台 | 玻璃与空间合成，舒展动作和共同释放 |

布景使用可编辑模块与明确地标，以轮廓、空间关系和动态装置区分世界；HDR 发光、曝光、Bloom 与材质一起校准，呈现亮核、彩色晕光、地面光池和角色轮廓光，控制脸部、UI 与提示曝光

粒子、轨迹、光带、冲击环和环境回应共用音乐段落与确认事件，关闭 Bloom 仅关闭泛光，装饰粒子另受画质和减弱动态设置控制；减弱闪烁与动作保留必要反馈

## 验收方法

实际执行清单见 [13 · 音乐表现](../todo/13-music-presentation.md)，达到商业参考作品的完成度由连续有声体验判断，截图数量与自动化测试数量不代表这一结论

本机已准备原创构造曲 `target/music-presentation-20261009/reference/original-80s.wav`，80 秒、48 kHz、PCM16 双声道，四个 20 秒音色段每两秒换和弦，32–36 秒安静、48–56 秒密集、68–70 秒静音、78–80 秒收束；`timeline.json` 给出构造真值，`authoring.json` 给出最终 48 kHz 坐标，仓库中的 [generate.cjs](../tools/music-presentation-check/generate.cjs) 向新目录重复生成，`receipt.json` 保存源码与文件 SHA-256

第一版生产导入保留在 `target/music-presentation-20261009/package`，包含 `song.audio.ogg`、`analysis.bin`、`chart.bin`、`song.package` 四个对象；共 3,840,000 帧、19 个手工 Anchor、4 个段落与 157 个 presentation 窗口，analysis 为 schema v3，来源 `Algorithm`、状态 `Candidate`、`confidence=None`，和弦与调性分数仍为未校准候选

该历史包 BLAKE3 为 `9a2fe898fdad9a370467ba40cb8291c38d0f2656dcb2e2d8c90b518547b2195c`，导入原始输出见 `target/music-presentation-20261009/import.log`；157 窗口仅属于第一版诊断，不作为新版算法和最终构建的验收输入

新版使用 `canonical-salience-8192-hop3072-v2-candidate`，8192 帧分析窗以 3072 帧、64 ms 步长推进；实际 Rust 生产导入 API 的预检包位于 `target/music-presentation-20261009/package-v2`，`import-v2.rs` 保留原样调用，`import-v2.log` 记录 1250 个窗口、315 个和弦候选和 299 个达到单窗口门限的窗口，候选窗口数不等于实际事件音高覆盖或正确率

正式 Lab CLI 已完成 `target/music-presentation-20261009/package-final` 导入，`analysis-final.json` 记录 1250 个窗口与内容身份 `package-blake3:ca2d87c159df79418c8fce71bd45c3858e2eae1d3f3846f512e66a2feabf16d9`；原生窗口性能与连续视频使用该最终包，预检包和历史包身份分别保留

该曲用于接线、回退和音高跟随检查，原曲听感与完整混音尚待试听；四个音色段用于覆盖候选分析，不要求单首歌自动切换四个世界

```sh
node tools/music-presentation-check/generate.cjs target/music-presentation-reference-new
target/debug/cocobeat-lab import-authored-package target/music-presentation-reference-new/original-80s.wav target/music-presentation-reference-new/authoring.json target/music-presentation-package-new
target/debug/cocobeat-lab inspect-stage-plan target/music-presentation-package-new
target/debug/cocobeat-game --package target/music-presentation-package-new
```

旧 `--feedback-motion-smoke DIR` 只输出 240 张软件控制的 GPU 帧；新版 [motion.cjs](../tools/music-presentation-check/motion.cjs) 使用完整歌曲的原生 GPU 连续帧、DuoEngine 确认事件和实际导入包查询的音频上下文，再用生产 Kira Renderer 离线渲染并合成有声预览，输出 `preview.mp4`、`frames/core-events.json`、`frames/events.json`、`audio/`、帧哈希与 `receipt.json`

```sh
node tools/music-presentation-check/motion.cjs /absolute/path/to/frozen/cocobeat-game /absolute/path/to/frozen/runtime-test target/music-presentation-20261009/package-final neon target/music-presentation-neon-motion-new
```

上述两个二进制须来自同一冻结源码的构建，第二项为包含 `feedback_audio::tests::export_reference_mix` 的 runtime 测试程序；world 可选 `neon`、`forest`、`candy`、`star-sea`，每次使用新输出目录，可由独立进程并行采样，最终四世界当前以四个进程推进；图像采样期间不并行性能测试，性能仍独占测量，原生图像加离线混音明确不属于扬声器回录

完整性能测量复用 `tools/runtime-performance-check/run.py`，使用冻结源码的优化构建回执、独占 GPU 与实际 Kira 播放，真实手柄、扬声器延迟和真人双人体验另验

原始音乐、纯反馈和混音必须来自同一事件时间表；实际录屏保留采集命令、音频来源、内容与二进制哈希，离线合成试听须明确标注，不能声称为设备回录

## 当前证据与未通过项

`render-v1` 至 `render-v4` 各已输出 14 张原生 GPU 静帧，原始命令、图片与二进制身份分别位于 `target/music-presentation-20261009/` 下对应目录的 `receipt.json`；捕获成功与画面评价分别记录

第一版因空间稀疏、表现平淡未通过，第二版因森林过暗、大面积发光材质缺少体积未通过；第三版已目检霓虹 6.3 秒及 Bloom 关闭对照、森林 6.3 秒、糖果 26.3 秒、星海 6.3 秒，物体体积、各世界雾色和空间层次改善，脸部与提示可读，但这五张检查不等于全场景、连续动效或商业艺术质量验收

进一步静帧检查发现玩家标签与耳尖重叠、cue 对比不足、溪流水面埋入地丘、双人反馈核心过亮和同心环过多，已分别调整；`render-v4` 的 14 张原生图为 PASS，主线程目检霓虹 6.3 秒、森林 26.3 / 46.3 秒、糖果 26.3 秒和 400×300 小窗口，确认这些具体问题的修正有效，连续动作与整体艺术观感仍另验

最终 Bloom 开启与关闭静帧已由主线程对照目检，关闭后必要反馈仍保留；低画质 400×300 样例的身份、cue 与必要反馈也已核对，这些指定样例通过不代替整首连续画面的可读性与艺术验收

本里程碑完整软件批次 `/tmp/cocobeat-presentation-final-tests.log` 记录全 workspace 515 项通过、0 失败、9 项显式忽略，其中 media 92 项、runtime 220 项；该批次位于上述最终画面修正之前，覆盖旧分析版本编解码、候选边界、大小调变化、静音噪声、反相、安静铺底与密集琶音、六音色缓存、未知和声回退、变体重建、短句和声交集、复音限制和设置保存，显式忽略项不计入通过数

画面修正后的最新 `/tmp/cocobeat-presentation-polish-final-tests.log` 记录 runtime 220 项通过、2 项显式忽略，最新 Clippy 与格式检查通过；最终 ponytail-review 移除 2 个始终隐藏的 Shock 实体后结论为 `Lean already. Ship.`，既有全量回归与本次局部回归分别保留

此前 `/tmp/cocobeat-presentation-workspace-final.log` 的 513 项全量批次与短尾修正后的 `/tmp/cocobeat-presentation-short-tail-tests.log` 的 runtime 220 项局部批次分别保留；最新 515 项来自重新执行完整 workspace，而不是相加旧批次的计数

最终 Clippy、格式与依赖边界检查均通过，日志分别为 `/tmp/cocobeat-presentation-final-clippy.log`、`/tmp/cocobeat-presentation-final-format.log`、`/tmp/cocobeat-presentation-final-boundaries.log`，提交前 ponytail-review 结论为 `Lean already. Ship.`；冻结优化构建 `release-final/cocobeat-game-build-result.json` 为 PASS，401 个输入保持一致，用时 459.60 秒，Game 二进制 SHA-256 为 `e68767f5b7fd956685002791186b88f0132a6976db99db4721299c0ff312e9b3`，构建路径位于 `target/music-presentation-20261009/`

汇总证据已持久化至[音乐表现观察记录](../testdata/synthetic/music-presentation-observations-20261009.json)，历史捕获与性能绑定各自构建身份，最终构建、四份视频及森林最忙场景性能结果均已归档

上一 `release-build` 为主动取消的构建，FAIL 回执保留但不归因为源码错误；`release-final` 性能基准 Game 已完成四世界原生真实窗口性能测量，均在 1920×1080、High、limited60 下通过实际 Kira 播放完整 80 秒并到达 Finished，软件输入经过正式规则流程，证据见 `target/music-presentation-20261009/full-song-performance.json`

| 世界 | 主更新间隔 p95 / ms | 不透明 3D pass GPU p95 / ms | 终态 |
|---|---:|---:|---|
| 霓虹城市 | 16.727077 | 5.69936 | Finished / 3,840,000 帧 |
| 发光森林 | 16.730295 | 6.85332 | Finished / 3,840,000 帧 |
| 糖果乐园 | 16.733460 | 5.73428 | Finished / 3,840,000 帧 |
| 星海剧场 | 16.733007 | 5.37756 | Finished / 3,840,000 帧 |

各世界记录 4782 个 Running 主更新间隔，均没有超过 33.333 ms 的间隔，20 / 40 / 60 秒段落边界前后 1.2 秒观察单独保留；主更新间隔包含 pacing 等待，不是 CPU 工作耗时或显示器 FPS，异步 GPU pass 查询也不代表整帧 GPU 时间，本机单轮结果不替代跨设备预算

四份 `motion-*` 目录的原 80 秒有声视频保留为缺少逐帧元数据的历史记录；此前把缩略图 HUD 读作 6.7 秒，放大原 `motion-neon/frame_0198.png` 与新 `motion-aligned-neon/frame_0198.png` 后，两者均明确显示 `006.6`，因此未证实原视频存在任何时间偏移故障，也不将原视频记为 FAIL

新增截图请求与保存握手用于加强采样证据，`frames.json` 逐样本记录请求和保存时的 SongTime，要求二者相等且严格按固定 30 fps 采样；`motion-aligned-*` 四份视频已全部 PASS，均为 80 秒、2400 帧，这是证据增强，不宣称修复已证实的同步故障

相对性能基准的冻结输入已核对，后续仅 `app.rs` 的 QA 截图握手与 `tools/music-presentation-check/motion.cjs` 改变，`release-final` 原生性能证据保持原身份有效；新的 `capture-release` 优化构建 PASS，用时 464.14 秒、401 个输入保持一致，二进制 SHA-256 为 `21cb577b6a663bdb32fdd7562f5a7c808eb186ff06b6a167a9a4022cfcddedbf`

随后标签、cue、水面和双人反馈画面已进一步修正，`capture-release` 与四份 `motion-aligned-*` 按原身份保留为历史；最终 `polish-release` 优化构建 PASS，用时 465.60 秒、401 个冻结输入保持一致，Game SHA-256 为 `7782f8480d333e0124437014e7c44e5243e4d965a09b021a7524d8aee99fd69d`

最终构建的森林最忙场景已完成独占 GPU、实际 Kira 的 80 秒整曲复验，结果 PASS，终态 Finished / 3,840,000 帧；主更新 p95 为 16.728882 ms、最大 16.767184 ms、无超过 33.333 ms 的间隔，不透明 GPU pass p95 为 6.86156 ms、RSS 为 469.574 MiB，记录位于 `target/music-presentation-20261009/polish-full-song-performance.json`，仍沿用主更新含 pacing、GPU pass 不等于总帧时间的口径

另外三个世界保留前述 `release-final` 原生性能基准，没有宣称最终三文件微调后又跑过原生性能；四份 `motion-final-*` 最终视频全部通过原生采样、同事件 Kira 离线混音和文件核验，各为 80 秒、1920×1080、30 fps、2400 帧，四进程采样时没有并行性能测量；每帧请求与保存的 SongTime 相等且对应 i×1600 音频帧，音视频流时长均为 80 秒，连续艺术表现、试听与商业艺术品质仍未验收

`audio-authored-control` 是旧参数和手工和弦上下文的离线控制，不作为新版实际歌曲接线证据；新版须从实际导入包与 GPU 捕获的 core 事件导出对应原曲、反馈与混音，保留解析上下文和来源

第一版实际 192 个事件没有任何音高层，促成分析步长与音级显著性修正；保守候选的独立 JS 构造真值诊断仅有 2 个音高事件，属于早期问题定位证据

新版 Rust 在实际 `package-v2` 上解析同一组离线编排事件，`resolved-events-adaptive.log` 记录 14 / 192 个事件具有音高层；专用 `short_tonal` 优先使用完整受保护的 0.985 秒长尾，长尾和声交集不足时回退到完整受保护的 0.545 秒短尾，接触音与密度选择保持原语义，不能把缩短检查区间当作允许尾音穿越未知和声

该 14 / 192 是已导入歌曲上的 Rust 上下文解析结果，尚不是物理输入或扬声器录制；`resolved-events-short.log` 使用误选的旧测试二进制，其 1 个音高事件结果不属于最终实现，真实曲库的音高覆盖与音乐准确率仍需提高并试听，当前不能宣称音高问题已完全解决

最终可玩入口为 `target/music-presentation-20261009/play.sh`，说明在同目录 `PLAY.md`；四个 `motion-final-neon`、`motion-final-forest`、`motion-final-candy`、`motion-final-star-sea` 目录各含 `preview.mp4`、三份 `audio/` WAV、逐帧时间、确认事件和来源哈希，属于原生画面与离线音频合成，未声称扬声器回录

最终确认事件样板每世界 163 个音效事件，其中 11 个带音高层、9 个使用受保护短尾；最大同时发声 4 个，反馈峰值 0.052–0.056、混音峰值 0.185–0.198，此样板未削波，真实曲库覆盖、独立听感与音乐正确率保持待验

试听包中的原曲 WAV 保留原始解码电平，混音采用音乐 0.4、反馈 0.8 增益，并为诊断保留一秒尾音；直接比较原曲与混音前应对齐播放响度，81 秒诊断 WAV 与 80 秒视频的长度差来自尾音保留
