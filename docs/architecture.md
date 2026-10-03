# 架构与依赖边界

箭头表示“依赖”，不表示运行时消息方向：

```text
game ───────────────→ runtime
runtime ────────────→ schema / core / replay / media
replay ─────────────→ schema / core
media ──────────────→ schema / Symphonia（源解码与严格读回）/ OxiMedia（重采样）/ Postcard（内容对象）
core ───────────────→ schema
schema ─────────────→ std
lab ────────────────→ 按实验需要使用上述模块
xtask ──────────────→ 开发检查工具
```

schema 定义整数时间、玩家/epoch/序号、Hit、水位和语义事件；core 的 DuoEngine 统一执行 Anchor 判定、一对一 Free Sync、Anchor Sync 与 Resonance；replay 保存带身份和版本的有界 JSON，再交给同一个 core 重放

runtime 的 app 组合 Bevy、Kira 和 Session，content 通过 media 加载已构建的歌曲包并适配为会话元数据和 Kira PCM；input 记录软件观察时刻，clock 映射 SongTime，session 维护规则事实、Replay 与诊断，audio 管理播放及错误，view 只消费表现状态；game 只调用 runtime 入口，lab 复用 clock 与原创 dev_song 生成器

media 当前负责有上限的 WAV/PCM、FLAC、MP3、Ogg Vorbis 顺序解码与固定 48 kHz 重采样，由 lab 的 `decode-audio` / `resample-audio` 实际消费；解码接口使用项目自有的采样率、实际帧数和立体声 PCM 块，单声道复制为双声道，拒绝非有限值及超限输入；源解码保留采样率、静默和幅度，重采样采用 OxiMedia High，48 kHz 样本直接透传，其他采样率核对实际输出帧数为整数 `ceil(源帧数 × 48000 / 源采样率)`

media 的 `decode_canonical` 复用同一顺序解码核心和 Ogg 页校验，由 lab 的 `readback-canonical` 实际消费；最终文件必须是 Ogg Vorbis、48 kHz、恰好双声道，EOS 声明和独立累计的实际帧数均须等于调用方提供的编码器输入帧数。输出从解码帧 0 连续交付，不做声道复制、重采样或裁幅，拒绝非有限样本；读回成功本身不证明源音频经过编码后的瞬态对齐或音质

schema 的 `AssetRef` 仅定义对象文件名、实际字节数和 BLAKE3，不依赖序列化或哈希库。media 的 `prepare_canonical_audio` 由 lab 的 `prepare-audio` 消费，流式复制最终 Ogg 至全新目录中的固定文件名，记录写入字节身份，关闭文件后严格读回该副本；返回 `PreparedCanonicalAudio`，不将它当作完整 `ValidatedPackage`。文件只在成功独占创建后才归本次清理，目录清理仅允许空目录；完整包由独立的 `build_package` 事务持有，先准备同一最终音频副本，再调用内容构建函数，写入分析、谱面和 manifest 并整体复核后重命名发布

源文件限 512 MiB、192 kHz 和十分钟，严格读回同样受文件大小和十分钟上限约束；库内 packet/block 上限不等于操作系统内存或 CPU 隔离。回调收到的块在整次操作成功前都是临时结果，失败必须丢弃；lab 只创建新输出并在错误时清理半成品，生产编码器准入与游戏中的完整导入流程尚未完成

schema 的初始 `MusicAnalysis` / `CompiledChart` / `SongPackage` 由 lab 构建手工内容包，再由 runtime 加载；能量从最终 staging Ogg 全量读回计算，Anchor 和段落来自有来源说明的创作 JSON，media 的私有 Postcard DTO 持有版本头、字节/元素限额、语义检查和对象身份，schema 仍仅依赖标准库

media 的 `read_package` 检查四对象，从同一份有界音频字节快照验证引用哈希并严格解码，将 PCM 块交给调用方，整次调用成功后才返回 `ValidatedPackage`；失败时调用方丢弃临时 PCM，`validate_package` 复用该路径并丢弃 PCM，完整契约见 [SongPackage](song-package.md)；校验不重新执行 MIR 或证明谱面合理性，runtime 消费音频、实际长度、包身份、Anchor 与 SectionCue，energy 和分析段落区间尚未驱动场景，后续 TempoRegion、重复结构、AnchorEvidence 和 TrackPlan 按实际消费者扩展并升级版本

`SongContent` 保留 chart 的点提示，app 在 Session 更新后派生最近 / 下一 cue，再把辅助字幕和下一时刻交给 view / scene；下一时刻严格晚于当前游标，同帧多项选择最高 ID，HUD 优先下一项、没有下一项才用最近项，scene 的固定三个门框实体仅在未来六秒内显示，按 `z = -3 × ahead` 移动，以上表现不进入 core 或 Replay 输入事实，也不修改音频生命周期与输入规则

`ui_assets` 复用既有六份 Noto Sans 字体，通过 Bevy 的 fontique 字体集合配置原生脚本回退；每个文本仍以 locale 对应的地区字体为首选，回退处理其缺少的拉丁 / 西里尔 / 希腊 / 汉字 / 假名 / 韩文字形，不引入系统字体依赖或任意 Unicode 覆盖承诺，字体类型止于 runtime 表现适配层

## 未来模块何时出生

| 模块 | 独立责任 | 引入时机 |
|---|---|---|
| media（已创建） | 源解码、重采样、严格最终读回与内容包事务；后续标准编码、完整 MusicAnalysis 与 AnchorCompiler | 05 的音频入口与构包已有 lab 消费，runtime 加载已构建包；其余按 05–07 的实际契约加入 |
| stage | MusicAnalysis / 编译后的音乐结构 → StagePlan，确定性轨道和几何校验 | 08 自动舞台；手写场景先在 runtime |
| editor | 波形、Anchor、Replay 的可视化与人工修改 | 09 已有可编辑内容契约 |
| net | Quinn 传输、会话、时钟映射、可靠输入历史和资源一致性 | 10 本地闭环与重放通过后 |

media / stage / net 允许依赖 schema，不能依赖 runtime；media 复用 schema 的唯一标准采样率。算法以项目自有类型为输入输出，第三方库类型止于适配器。runtime 组合实现；game 只保留配置和启动，不承载算法。未来增加 crate 时必须说明责任、依赖和失败方式，并更新边界检查。

## 事实流

```text
Bevy 输入消息 → ClockBridge → Hit / 水位 → core::DuoEngine → 语义事件
                                   ↑                          ↓
                          Replay / 未来网络            app 声光反馈 → view
```

相同输入历史、规则版本、内容和 epoch 必须得到相同规则结果；Replay 不另写判定算法，未来网络到达时间不能改写原始输入时间，view 不获得规则引擎的可变控制能力

正常启动接受 `--package DIR`，也可追加 `--replay FILE` 或 `--visual-smoke PNG` 校验该包的 Replay 或生成无音频场景预览；`--section-smoke FRAME CODE PRESET WIDTH HEIGHT SCALE PNG` 用同一 cue 查询生成指定帧的无音频预览，帧范围包含 EOF，完整参数见 [SongPackage](song-package.md)；加载失败或规则不是 `duo-watermark-v1` 时退出，无参数正常启动才选择确定性生成的 64 秒开发歌曲，原有不带包的 Replay 与视觉诊断入口保留开发内容

runtime 以完整 manifest 的 `package_hash` 构造 `package-blake3:<64 个十六进制字符>` 内容身份，Session 使用包的实际结束帧与 Anchor；Replay 记录内容/规则/构建身份和原始输入、水位，保存失败明确报错，校验与大小上限由 replay 持有，runtime 另检查 Hit 是否处于歌曲范围内；诊断 CSV 由 session 写入，二者不包含歌曲音频

包的 PCM 在启动时一次加载为 Kira `StaticSoundData`，整首帧数组由 `Arc` 持有，开始和重开歌曲共享该数组；普通包启动仍完整播放品牌开场，随后停在 Ready，用户显式 Start 才播放歌曲，开场期间的控制由既有输入屏障隔离

## 自动约束

`cargo xtask boundaries` 检查 Cargo metadata 中所有直接依赖声明，包括 build/dev、目标平台条件与重命名依赖。
schema / core 不允许外部依赖，replay 仅允许 serde / serde_json 处理私有持久化格式，且只能沿上图依赖；game 的运行时只允许 runtime，Windows 构建脚本允许 embed-resource 编译 EXE 图标资源，例外不扩展到普通、dev 或其它平台依赖。media、runtime 和 lab 可接入第三方实现依赖；未声明的本地 helper 不得绕过边界
需要 serde 等纯数据工具时，应显式更新白名单并说明用途，不能泛化为允许任意第三方依赖。

边界检查约束模块图，不能证明所有函数都尊重语义；例如反馈不修改判定，还需要 API 设计、测试和代码审查。
