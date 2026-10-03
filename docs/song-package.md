# 初始 SongPackage 契约

当前交付覆盖最终音频、实测能量、手工 Anchor / SectionCue 与四文件包的构建和校验，入口为 `cocobeat-lab build-authored-package` 与 `verify-package`；运行时仍使用现有开发歌曲，本入口不启用自动 MIR、AnchorCompiler、StageCompiler 或生产编码器

字段与校验以 [schema/content.rs](../crates/cocobeat-schema/src/content.rs)、[content_codec.rs](../crates/cocobeat-media/src/content_codec.rs)、[media/package.rs](../crates/cocobeat-media/src/package.rs) 和 [lab/package.rs](../tools/cocobeat-lab/src/package.rs) 为准，当前 `CONTENT_SCHEMA_VERSION` 为 `1`

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

## 构建、发布与失败清理

构建在目标目录的同一父目录下创建本次专用的 `.cocobeat-package-<pid>-<序号>` staging，复制音频时对实际写入字节计算长度与 BLAKE3，同步文件后严格回读副本；内容构建回调收到该副本路径和 `PreparedCanonicalAudio`，lab 从这个确切路径测量能量，并用已准备音频的身份生成分析和谱面引用，后续不依赖对原来源路径的重复读取

Rust 入口为 `build_package(source_audio, expected_frames, destination, build_content)`，`build_content` 是返回 `PackageBuildInput` 的一次性回调；帧数由独立参数和严格回读决定，`PackageBuildInput` 提供歌曲身份、版本、分析和谱面，成功返回已验证的 `ValidatedPackage`

三个元数据文件用 `create_new` 写入并分别 `sync_all`；完整 staging 通过 `validate_package` 后，再检查目标不存在并以同一文件系统内的目录 `rename` 发布，目标目录一次出现完整四对象；检查时已存在的目标，包括空目录和符号链接，都会被拒绝，本工具并发构建产生的非空获胜目录也不会被后续构建替换

失败只删除本次明确创建的文件，再尝试移除空 staging 目录；其他写入者留下的文件保留，清理失败会追加到错误信息，不递归清除目录，也不修改源音频；未完成 staging 不能通过四对象验证，此处原子发布指目录可见性，不宣称断电后的目录持久化保证

## 验证范围与后续准入

`validate_package` 先验证目录和有界对象，再验证头、载荷、schema 语义、对象长度、完整字节哈希及交叉音频引用，最后通过 `decode_canonical` 完整读取最终 Ogg，检查单轨 Ogg Vorbis、48 kHz、双声道、有限样本、Ogg 页 CRC / 顺序 / EOS、从帧 0 开始的连续时间线以及声明和实际总帧数；严格路径不重采样、不复制单声道、不裁幅、不自动删头尾静默

包格式验证成功只证明当前初始契约及最终音频结构通过，不等于编码音质、seek、设备兼容、真人听感或运行时曲库 Ready 已通过；编码候选的独立状态继续见 [canonical 音频实验](canonical-audio-probe.md)

[06 MIR 基准](../todo/06-mir-benchmark.md) 的完整 MusicAnalysis 能力、合格检测器与置信度依据仍待交付；[07 AnchorCompiler](../todo/07-anchor-compiler.md) 的 AnchorEvidence、接受 / 拒绝原因与生成策略尚未由手工 Anchor 替代；[08 StageCompiler](../todo/08-stage-compiler.md) 的 TrackPlan / StagePlan 及确定性关键几何仍待后续，本包当前没有对应舞台对象
