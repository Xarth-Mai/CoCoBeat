# 初始 SongPackage 契约

当前交付覆盖最终音频、实测能量、手工 Anchor / SectionCue 与四文件包的构建、校验和 Anchor 编辑导出，lab 入口为 `build-authored-package`、`verify-package` 与 `edit-anchors`；runtime 可通过 `--package DIR` 加载包中的音频、实际长度、Anchor 与 SectionCue，并从真实分析区间编译直道 / 广场 / 缓弯 / 低桥 StagePlan，仍未接入自动 MIR、AnchorCompiler、完整 StageCompiler 或生产编码器

字段与校验以 [schema/content.rs](../crates/cocobeat-schema/src/content.rs)、[content_codec.rs](../crates/cocobeat-media/src/content_codec.rs)、[media/package.rs](../crates/cocobeat-media/src/package.rs) 和 [lab/package.rs](../tools/cocobeat-lab/src/package.rs) 为准，运行时适配见 [runtime/content.rs](../crates/cocobeat-runtime/src/content.rs)，当前 `CONTENT_SCHEMA_VERSION` 为 `1`

## 四个固定对象

一个包是只包含下列四个普通文件的目录，文件名固定；校验拒绝额外目录项、缺失对象、符号链接根目录或对象，以及引用长度、哈希或内容不符

| 文件 | 内容 | 完整文件大小上限 |
|---|---|---|
| `song.audio.ogg` | 已完成编码的 Ogg Vorbis，48 kHz、恰好双声道 | 512 MiB |
| `analysis.bin` | `MusicAnalysis` 初始音乐事实 | 16 MiB |
| `chart.bin` | `CompiledChart` 手工 Anchor 与段落提示 | 4 MiB |
| `song.package` | `SongPackage` manifest、对象引用与包身份 | 64 KiB |

`canonical_frames` 必须在 `1..=28_800_000` 内，即最多十分钟；事件时间用从最终解码音频第 0 帧起的整数 `SongTime` 表示，一帧含左右声道两个样本，所有事件时刻必须满足 `0 <= frame < canonical_frames`

`SongPackage` 保存 `schema_version`、`song_id`、三个 `AssetRef`、采样率、声道数、总帧数、`importer_version`、`analysis_version`、`chart_version` 和 `package_hash`；每个 `AssetRef` 保存固定文件名、非零 `byte_len` 和 32 字节 BLAKE3

## 二进制头、载荷与哈希

三个元数据对象都使用 20 字节头和 Postcard 载荷，大小上限包含头；音频对象保留原 Ogg 字节，不加此头

| 偏移 | 长度 | 定义 |
|---|---|---|
| `0` | 8 字节 | `analysis.bin` 为 `CCANLYS\0`，`chart.bin` 为 `CCCHART\0`，`song.package` 为 `CCPACKG\0`，末尾均为一个 NUL 字节 |
| `8` | 4 字节 | little-endian `u32` schema 版本，当前为 `1` |
| `12` | 8 字节 | little-endian `u64` 载荷长度，不含头 |
| `20` | 指定长度 | Postcard 载荷，按 codec 的字段顺序编码 |

读取时要求对象 magic 匹配、版本受支持、头内长度等于实际剩余字节数、Postcard 恰好消费完整载荷，并再次调用 schema 的语义校验；截断、尾随数据、超限或不支持的头内 / 载荷内版本均失败，载荷不依赖 Rust 内存布局

`AssetRef.blake3` 对引用文件的完整原始字节计算，包含 `analysis.bin` / `chart.bin` 的 20 字节头；`MusicAnalysis.audio_hash` 与 `CompiledChart.audio_hash` 都必须等于 `manifest.audio.blake3`

`package_hash` 的输入是完整 manifest 编码：保留所有字段，仅把 `package_hash` 字段替换为 32 个零字节，再按同一 codec 编码头和 Postcard 载荷，对全部编码字节计算 BLAKE3；最终 `song.package` 写入该结果，验证时重新归零计算，不对含最终哈希的文件直接做自哈希，也不只对载荷计算

各个事件 / 特征集合分别最多 100,000 项；标识符和标签为 1–256 个 UTF-8 字节，诊断文本最多 4096 个 UTF-8 字节；解码在取得集合项和复制文本时执行边界检查，不按未经验证的集合长度预分配

## 初始分析与谱面

`MusicAnalysis` 已区分 `beats`、`onsets`、`sections`、`energy` 与 `diagnostics`，beat / onset 带强度和可选置信度，beat 另有可选 downbeat 概率；强度要求有限且非负，可选概率为 `None` 或有限的 `0..=1`，`None` 表示未测量

beat / onset 时间分别严格递增；分析段落使用 `[start, end)`，要求非空、有序且不重叠，`end` 可以等于音频总帧数；段落之间可以留空，不要求覆盖整首音频

lab 从本次 staging 中已严格校验的最终 Ogg 副本逐声道计算能量，每块 1024 帧，最后一块使用实际剩余帧数；RMS 以 `f64` 累加平方后计算并转为 `f32`，peak 为实际样本绝对值最大值，能量块从帧 0 连续覆盖全部音频；能量必须有限、非负且逐声道满足 `rms <= peak`，不把合法的有限峰值强制裁到 1

此 CLI 的 `beats` 和 `onsets` 保持空集，分析段落来自手工 JSON，段落 `confidence` 为 `None`；`diagnostics` 明确记录能量测量方式、未执行 beat / onset 检测以及手工来源，版本标记为 `canonical-rms-1024-v1` 和 `manual-anchors-v1`，导入器标记为运行该命令的 `cocobeat-lab/<版本>`

`CompiledChart` 保存 `ruleset_id`、`anchors` 与 `sections`；Anchor ID 在 Anchor 集合内唯一，SectionCue ID 在 SectionCue 集合内唯一，各集合按 `(time, id)` 严格递增，同一帧允许不同 ID，Anchor ID 与 SectionCue ID 不共用唯一性范围；SectionCue 只记录手工段落的开始帧和标签，完整结束帧保留在分析段落中

校验器检查这些字段的范围、顺序、覆盖与音频引用，不重新计算 energy 的测量值，也不评估手工 Anchor 的音乐合理性；当前 lab 构建路径负责执行真实能量测量，通用 `PackageBuildInput` 的调用方负责提供其分析事实

## 手工 authoring JSON

顶层字段和子对象字段均固定，未知字段会被拒绝；文档上限为 1 MiB，`schema_version` 必须为 `1`，`source_note` 去除首尾空白后须非空，原文本长度最多 2048 个 UTF-8 字节；时间、ID、排序、标签和集合大小沿用上述 schema 校验，CLI 不自动排序、补 Anchor 或修正越界值

下面是一份用于一秒、48,000 帧最终音频的完整示例，实际歌曲须按自己的最终音频帧数编写

```json
{
  "schema_version": 1,
  "song_id": "authored-example",
  "ruleset_id": "duo-watermark-v1",
  "source_note": "Manually authored against the final decoded audio",
  "anchors": [
    { "id": 1, "frame": 12000 },
    { "id": 2, "frame": 36000 }
  ],
  "sections": [
    { "id": 1, "start_frame": 0, "end_frame": 48000, "label": "opening" }
  ]
}
```

仓库的 [开发歌曲 authoring.json](../assets/dev/vertical_slice/authoring.json) 对应 64 秒、3,072,000 帧，保留七个手工 Anchor 和六个段落；其来源是同目录的 `anchors.csv` 与 `event_frames.csv`，不是自动音乐检测输出

## CLI 使用

命令从仓库根目录执行，目标父目录必须存在，目标包目录必须尚不存在；输入 `final.ogg` 应是已完成编码的最终文件，`expected-frames` 来自编码前实际输入长度或已有可信音频记录，不能只抄待验证 Ogg 的声明帧数

```sh
cargo run --locked -p cocobeat-lab -- build-authored-package /path/to/final.ogg 48000 /path/to/authoring.json /path/to/new-song-package
cargo run --locked -p cocobeat-lab -- verify-package /path/to/new-song-package
```

构建命令读取 authoring、准备最终音频副本，再测量该副本的能量并完成四文件包；验证命令重新读取已有包，成功时输出总帧数、能量块 / Anchor / SectionCue 数量和包 BLAKE3，任何失败返回错误退出码，完整参数见 [lab/main.rs](../tools/cocobeat-lab/src/main.rs)

当前工作区已有的 `target/fullband-regression-20261003/cases/dev-song-64s/baseline/canonical.ogg` 是原创 64 秒音乐的开发编码候选，可用于下列本地包格式实验；该路径是未随仓库分发的实验产物，不能据此视为生产编码器准入，其他环境可提供自己已完成的最终 Ogg 和对应 authoring / 帧数

```sh
cargo run --locked -p cocobeat-lab -- build-authored-package target/fullband-regression-20261003/cases/dev-song-64s/baseline/canonical.ogg 3072000 assets/dev/vertical_slice/authoring.json target/manual-duet-package
cargo run --locked -p cocobeat-lab -- verify-package target/manual-duet-package
```

仅需音频对象时可用 `prepare-audio <final.ogg> <expected-frames> <new-staging-dir>`，它只生成 `song.audio.ogg`，不等于完整包；需要导出严格回读 PCM 时可用 `readback-canonical <final.ogg> <expected-frames> <new-output.f32le>`，它不会自动创建分析或谱面

编辑已有包使用 `edit-anchors PACKAGE PATCH NEW_PACKAGE`，对应 Rust 入口为 `export_anchors(source, expected_package_hash, anchors, destination)`；它保留音频和分析原字节，仅在 Anchor 实际改变时重写 chart 与 manifest 身份，无变化时保留四对象及原身份，目标须为源包外的新目录；补丁、撤销重做与原 Replay 的身份边界见 [Anchor 命令行编辑](editor.md)，时间线 UI 仍未实现

## 运行时加载与会话

游戏入口接受 `--package DIR`，包参数必须位于可选模式之前；普通模式进入游戏，追加 `--replay FILE` 校验该包的本地 Replay，追加 `--visual-smoke PNG` 按该包的实际长度、Anchor 与 SectionCue 生成无音频场景预览

```sh
cargo run --locked -p cocobeat-game -- --package /path/to/song-package
cargo run --locked -p cocobeat-game -- --package /path/to/song-package --replay /path/to/replay.json
cargo run --locked -p cocobeat-game -- --package /path/to/song-package --visual-smoke /path/to/scene.png
```

runtime 在创建游戏和音频输出前完成 `media::read_package`，检查全部对象并收集严格解码得到的 48 kHz 双声道 PCM；任何读取、校验或解码失败都会退出，`ruleset_id` 当前只接受 `duo-watermark-v1`，未知规则也会退出，不改用开发歌曲

加载成功后，PCM 一次转换为 Kira `StaticSoundData`，完整帧数组保存在 `Arc` 中；开始和重开歌曲共享这份 PCM，不重新读取包或解码音频，当前播放路径将整首歌曲留在内存中

`SongContent` 取 manifest 的 `canonical_frames` 作为实际结束时刻，取 chart 的 Anchor 构造 Session，音频游标、Hit 边界、结束确认与进度显示使用该长度；Replay 的内容身份为 `package-blake3:` 加完整 64 个十六进制字符的 `package_hash`，绑定整个规范 manifest 及其对象引用，同一音频配不同谱面也使用不同身份，校验拒绝内容身份不匹配和歌曲范围外的 Hit

无参数正常启动继续使用确定性生成的 64 秒开发歌曲，原有不带包的 Replay 与视觉诊断入口也保留开发内容；正常包启动完整播放品牌开场，结束后保持 Ready，用户显式选择 Start 才开始歌曲，开场期间按住的控制不能穿透到演奏

`SongContent` 同时保留 chart 的 `sections`，按 Session 的整数游标查询最近 `time <= 当前帧` 的 cue 和严格未来 `time > 当前帧` 的下一 cue，同一时点多项统一选择 ID 最大的一项；HUD 优先显示下一提示，没有下一项才显示最近提示，不把下一 cue 当成当前 cue 的结束帧，也不从分析区间的空隙推断段落或置信度

段落文案复用 Logo 下的辅助字幕，显示本地化前缀、cue ID 与作者标签；显示层将控制字符和连续空白折为单行，纯空白标签只显示 ID，长行在限定区域裁剪，原始标签与包身份不变；菜单打开、不处于 Running / Pausing / Paused 阶段或到达 EOF 时清空动态文案并恢复原氛围字幕，逻辑宽度小于 900 或高度小于 600 的 compact 布局仍隐藏整行

下一 cue 的空间预告使用固定三个实体组成的门框，仅在 `0 < ahead <= 6` 秒时出现；歌曲包按 StagePlan 的整数距离差定位，无参数开发场景沿用 `z = -3 × ahead`；位置跟随现有歌曲游标，暂停时冻结，到点后查询下一 cue，不产生按键要求、Anchor 判定、输入事实或额外音频

字幕使用既有六份 Noto Sans 字体，界面 locale 继续决定首选地区字形，缺少字符时通过原生 fontique 按脚本回退，涵盖英文界面中的已有 CJK / 韩文字集以及 CJK 界面中的乌克兰字母；此处不承诺任意 Unicode、emoji 或扩展汉字覆盖

开发歌曲在代码中保留与现有 authoring 一致的六个固定 cue：0、8、24、40、48、60 秒，生产启动不读取 authoring JSON，且继续使用没有 StagePlan 的原手写场景；歌曲包的分析区间已驱动下述地面计划，`energy` 尚未驱动场景，这不等于完整自动音乐分析与舞台编译管线

指定整数帧的无音频预览入口为 `--package DIR --section-smoke FRAME CODE PRESET WIDTH HEIGHT SCALE PNG`；`FRAME` 接受 `0..=canonical_frames`，EOF 用于观察提示清空和终点，`CODE` 必须是已支持的完整语言代码，`PRESET` 为 `low`、`medium`、`high` 或 `off`，宽高使用物理像素，`SCALE` 为 DPI 缩放；预览复用生产 cue 查询和地面计划并输出 PNG 与 `CONTENT_SAMPLE` 状态，其中 `stage` 子对象包含编译版本、片段数、采样帧、类型、距离、半宽、侧向位移、抬升、两个切线分量和 `at_end`，完整内容身份保留在外层，不代替音频或物理输入验收

```sh
cargo run --locked -p cocobeat-game -- --package /path/to/song-package --section-smoke 0 en-US high 1280 720 1 /path/to/section.png
```

歌曲包的坡面反馈预览使用 `--package DIR --feedback-smoke FRAME EFFECT PRESET WIDTH HEIGHT SCALE PNG`，`EFFECT` 沿用 `local/free/anchor/anchor-good/miss/approach`；复用同一包加载、场景和反馈材质，输出 `CONTENT_SAMPLE` 与 `FEEDBACK_SAMPLE`，`approach` 使用包内真实下一 Anchor。此入口是固定状态取帧，不模拟音频或真实玩家输入

## 内存 StagePlan 与整数采样

[cocobeat-stage](../crates/cocobeat-stage/src/lib.rs) 的 `compile(content_id, end, sections)` 消费真实 `analysis.sections`，短于 16 秒的合法区间生成 Plaza，至少 16 秒的区间以前半段 Curve、后半段 Bridge 编排，奇数帧中点向下取整；首尾和区间之间的空隙生成 Straight，空分析列表生成一条全曲 Straight；输出非空、有序、连续覆盖 `[0, end)` 的片段，最多从 N 个区间生成 `2N + 1 + min(N, floor(end_frames / 768000))` 个片段，拒绝超限、倒序、重叠和越界输入，不按 chart cue 补区间，也不使用标签或置信度来选择几何

计划的 `sample` 接受 `0..=end`，范围外返回无采样；纵向距离为 `floor(frame / 16)` 毫米，对应 48 kHz 下 3 m/s，基础半宽为 3500 mm；Plaza 的名义峰值为 `peak = min(500, floor(duration_frames / 32))` 毫米，额外半宽为 `floor(2 × peak × min(elapsed, duration_frames - elapsed) / duration_frames)`，形成对称三角拓宽，首尾回到基础宽度，奇数帧区间的整数中点不保证达到名义峰值，少于 32 帧的区间保持基础宽度

Curve 侧向峰值为 600 mm，Bridge 抬升峰值为 300 mm，基础半宽保持 3500 mm；两者使用 `16 × A × u² × (1-u)²`，其中 `u` 是片段内的归一化时间，位置与斜率 ppm 用 i128 有理数计算，最近整数舍入、半值远离零；解析中心线在端点位置和一阶导数均回到零，毫米 / ppm 输出仍是整数台阶，Plaza 路宽仍是分段线性，纵向距离没有替换成弧长

EOF 采样保留最后片段的类型、返回基础半宽、零位移 / 坡度和实际结束距离；包加载在创建游戏及音频输出前编译一次，并以 `Arc<StagePlan>` 共享，重开不重编译，计划身份绑定完整 `package-blake3:<64 个十六进制字符>` 与 `compiler_version = 2`

StagePlan 是由包派生的内存对象，不是第五个包文件，四对象 content v1 与 Replay v1 保持原格式；相同内容和编译版本的计划与整数采样可复现，现有 Replay 没有舞台编译版本选择器，规则回放成功不代表跨版本视觉重放已实现

runtime 固定复用九个动态网格：路面、两侧地面、两条路缘，以及只在 Bridge 区间绘制的两面桥侧墙和两条低护栏；另有两个固定霓虹拱门实例，终点仍为地面标线。窗口为当前显示位置前 42 m、后 12 m，最多 257 个横断面，64 条基础条带、歌曲首尾和可见 Curve / Bridge 的起点 / 中点 / 终点优先保留，剩余预算才补短 Plaza 截面；极密短段按基础采样近似，完整 StagePlan 保留，实体和网格数量不随段落数增长

地面、标线、Anchor、cue 和拱门按同一当前 / 目标整数采样计算三轴相对位置，镜头保持固定朝向，角色仍位于中心两侧；桥坡上的反馈环随坡面倾斜，共享环在包场景固定抬高 12 cm 以跨过曲率，Precise / Good 保持相同运动时长及各自强度。建筑内侧退到至少 5.7 m，避免弯道路肩穿入近景；拱门是低亮装饰，不产生 Anchor 或判定

窗口在歌曲范围外延伸的地面按基础宽度绘制，只作场景衬底，不增加可演奏帧或 Hit；地面、标线与终点的显示游标夹到 `0..=end`，原始 Session 时间和预告有效性判断保持原样，Anchor / cue 的四秒 / 六秒窗口不变；画质与 Resonance 不修改计划或关键采样，品牌 Ready、手柄组合、输入门控和音频生命周期沿用现有流程

lab 的 `inspect-stage PACKAGE FRAME` 先完整验证包，再输出一行稳定 JSON，字段为 `content_id`、`compiler_version`、`segment_count`、`end_frames`、`frame`、`kind`、`distance_mm`、`half_width_mm`、`lateral_mm`、`elevation_mm`、`slope_x_ppm`、`slope_y_ppm`、`at_end`；`FRAME` 是 `0..=canonical_frames` 的整数，`kind` 为 `straight`、`plaza`、`curve` 或 `bridge`，结果不含机器路径、当前时间或音频字节，入口见 [lab/stage.rs](../tools/cocobeat-lab/src/stage.rs)

```sh
cargo run --locked -p cocobeat-lab -- inspect-stage /path/to/song-package 0
```

## 构建、发布与失败清理

构建在目标目录的同一父目录下创建本次专用的 `.cocobeat-package-<pid>-<序号>` staging，复制音频时对实际写入字节计算长度与 BLAKE3，同步文件后严格回读副本；内容构建回调收到该副本路径和 `PreparedCanonicalAudio`，lab 从这个确切路径测量能量，并用已准备音频的身份生成分析和谱面引用，后续不依赖对原来源路径的重复读取

Rust 入口为 `build_package(source_audio, expected_frames, destination, build_content)`，`build_content` 是返回 `PackageBuildInput` 的一次性回调；帧数由独立参数和严格回读决定，`PackageBuildInput` 提供歌曲身份、版本、分析和谱面，成功返回已验证的 `ValidatedPackage`

三个元数据文件用 `create_new` 写入并分别 `sync_all`；完整 staging 通过 `validate_package` 后，再检查目标不存在并以同一文件系统内的目录 `rename` 发布，目标目录一次出现完整四对象；检查时已存在的目标，包括空目录和符号链接，都会被拒绝，本工具并发构建产生的非空获胜目录也不会被后续构建替换

失败只删除本次明确创建的文件，再尝试移除空 staging 目录；其他写入者留下的文件保留，清理失败会追加到错误信息，不递归清除目录，也不修改源音频；未完成 staging 不能通过四对象验证，此处原子发布指目录可见性，不宣称断电后的目录持久化保证

## 验证范围与后续准入

`read_package` 先验证目录和有界对象，再验证头、载荷、schema 语义、对象长度、完整字节哈希及交叉音频引用；音频读入一份有大小上限的字节快照，对这同一份字节验证引用哈希并通过 `decode_canonical_bytes` 严格解码，不在校验哈希后重新打开路径取得播放内容

严格解码检查单轨 Ogg Vorbis、48 kHz、双声道、有限样本、Ogg 页 CRC / 顺序 / EOS、从帧 0 开始的连续时间线以及声明和实际总帧数，不重采样、不复制单声道、不裁幅、不自动删头尾静默；交给回调的 PCM 在整个调用成功前均为临时结果，失败必须丢弃，runtime 仅在成功后将收集的 PCM 交给音频输出，`validate_package` 复用相同读取路径并丢弃 PCM

包格式验证成功只证明当前初始契约及最终音频结构通过，不等于编码音质、seek、设备兼容、真人听感或游戏内曲库导入流程已通过；编码候选的独立状态继续见 [canonical 音频实验](canonical-audio-probe.md)

[06 MIR 基准](../todo/06-mir-benchmark.md) 的完整 MusicAnalysis 能力、合格检测器与置信度依据仍待交付；[07 AnchorCompiler](../todo/07-anchor-compiler.md) 的 AnchorEvidence、接受 / 拒绝原因与生成策略尚未由手工 Anchor 替代；[08 StageCompiler](../todo/08-stage-compiler.md) 已有手工区间派生的直道 / 广场 / 缓弯 / 低桥、霓虹拱门和终点，完整自动编排、真人预告可读性和跨版本视觉重放仍待后续，当前包没有持久化舞台对象

## 原始音频的手工内容导入

`import-authored-package SOURCE_AUDIO AUTHORING_JSON NEW_PACKAGE` 从有界源快照经唯一准入编码器创建上述四对象，自动取得真实最终 N 和来源诊断；Anchor / 段落仍由作者以最终 48 kHz 整数帧提供，具体操作、故障保护与能力边界见[源导入](source-import.md)
