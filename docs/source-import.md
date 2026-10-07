# 手工内容的生产源导入

`cocobeat-game --import-authored SOURCE_AUDIO AUTHORING_JSON NEW_PACKAGE` 将有界源音频经唯一的 `encode_canonical_audio` 路径转换成完整四对象包，完整校验后播放开场并停在 Ready，等待新的开始确认；lab 的 `import-authored-package` 使用同一导入实现，已制作的包也可由 `--package NEW_PACKAGE` 加载；authoring 使用[四对象契约](song-package.md)规定的手工 Anchor / 段落，当前游戏规则为 `duo-watermark-v1`

## 正常启动的 Ready 导入

无参数启动完整播放品牌开场并停在 Ready；打开「选择歌曲」，再选择「导入手工歌曲」，源文件扫描使用当前曲库的 `imports` 子目录，不打开系统文件选择器

默认曲库在 Linux 为绝对 `XDG_DATA_HOME/cocobeat/songs`，缺少有效 XDG 路径时为 `HOME/.local/share/cocobeat/songs`；Windows 为绝对 `LOCALAPPDATA/CoCoBeat/Songs`，可用 `--library DIR` 指定曲库；界面显示实际路径，目录缺少时请先自行准备曲库及 `imports`，游戏不自动创建这两个目录

1. 将音频和同名作者文件放在 `imports` 的直接子层，例如 `song.wav` 配 `song.authoring.json`，`song.flac`、`song.mp3` 或 `song.ogg` 也按同样规则配对；扩展名不区分大小写，只有双方都是普通文件才列为候选，符号链接、递归子目录及缺少配对的源不进入列表
2. 在导入页刷新并选中一对文件，确认页显示来源音频、作者 JSON 和将要新建的曲库子目录；候选名称只表示发现了配对，完整源解码和作者内容校验在确认后执行
3. 选择「确认导入新歌曲包」，单个自有后台 worker 复用生产编码器和四对象事务，再复用完整 package loader、PCM 与 StagePlan 加载；已有目标拒绝覆盖，导入按界面显示的新目录发布
4. 成功且仍在本地有焦点的导入页时，先保存旧 Replay，再一起换入新歌曲、PCM 和舞台，保留玩家绑定并返回 Ready；持键须释放后重新确认开始，导入本身不开始演奏

源候选扫描接受至多 512 个直接目录项和 128 对文件，超过任一上限会拒绝本次扫描，名单不是格式或质量准入；生产解码只接受 WAV / PCM、FLAC、MP3、Ogg Vorbis 的单轨单声道或双声道，源文件限 512 MiB、采样率不超过 192 kHz、长度不超过十分钟，作者 JSON 限 1 MiB；损坏或不受支持的实际内容在后台校验时显示错误

导入页复用现有菜单主控、焦点滚动和持键屏障，双手柄、键盘加手柄组合保持原玩家绑定；主控用方向键或手柄十字键 / 摇杆导航、Enter / 南键确认、Esc / 东键返回，其他设备用 Enter / Start 接管菜单时首次仅接管，具体设备行为见 [输入约定](platform-input.md)

确认前返回只撤回选择；确认后「返回 · 导入继续」或失焦只放弃本次自动换歌，源导入仍在后台完成，旧歌曲与 Replay 保留，完成前 Ready 的开始 / 重开均不可用，完成后可刷新曲库选择已保存的包；后台错误保留原原因而不是改成成功提示，关闭窗口等待自有 worker 结束，不分离仍在写包的线程

手工作者文件按最终 48 kHz 整数帧提供 Anchor / 段落，新分析为 `canonical-rms-1024-v2` 的实测能量和手工事实，自动 MIR 与自动 Anchor 没有在此入口运行；若包已合法发布但完整 loader 拒绝未知规则等内容，界面错误包含已保存路径，保留该包和原歌曲，不能把失败解释为没有创建输出

## CLI 导入

```sh
cargo run --locked -p cocobeat-game -- --import-authored /path/to/source.wav /path/to/authoring.json /path/to/new-package
cargo run --locked -p cocobeat-lab -- verify-package /path/to/new-package
cargo run --locked -p cocobeat-game -- --package /path/to/new-package
```

共享 Rust 入口为 `cocobeat_media::import_authored_package(source, authoring_path, destination, importer_version) -> Result<ValidatedPackage, String>`；`build_authored_package` 从已完成的 canonical 音频构建手工包，游戏和 lab 分别传自己的工具版本，lab 只保留薄适配，源格式和采样率由既有 Symphonia 源解码器检查，支持 WAV / PCM、FLAC、MP3、Ogg Vorbis 的单轨单声道或双声道输入，源长不超过十分钟，源文件不超过 512 MiB；输入须为普通文件，符号链接不作为此入口的源文件

工具先在目标的父目录创建本次独占暂存目录，以 8192 字节块复制源音频并计算实际快照的 BLAKE3，编码只读取这个快照；导入后修改原来源文件不影响已发布包，来源哈希描述实际复制的字节，不代表原文件在复制期间从未被修改

编码器返回实际源采样率、源帧数和最终 48 kHz 帧数，工具不要求用户估算 N，也不裁切、补帧、归一化或替换超出编码数值域的音频；严格最终读回通过后，既有 `build_package` 再复制并严格回读最终 Ogg，从这个确切副本测量能量，验证全部四对象后一次性发布新目录

`analysis.diagnostics` 保留来源快照的 BLAKE3、字节数、源采样率、源 N、canonical N、编码 profile 及作者 `source_note`，`manifest.importer_version` 记录实际调用方 `cocobeat-game` 或 `cocobeat-lab` 的版本和唯一 encoder profile；实际最终 Ogg 的长度与 BLAKE3 继续由 manifest 的音频对象引用记录，包 hash 绑定这些元数据和谱面身份

Anchor / 段落坐标是最终音频的 48 kHz 整数帧，工具不将源采样率坐标自动换算成谱面；越界、重复或倒序内容由现有 schema 校验拒绝；beat / onset 检测没有运行，段落置信度保留 `None`，能量是真实最终 PCM 的 1024 帧 RMS / peak，CLI 与 Ready 的配对导入页消费同一手工事务，自动 MIR 与完整 Anchor 策略继续独立推进

已有目标目录、文件和符号链接均拒绝，失败尝试删除本次明确创建的源快照、编码输出和空暂存目录，并保留原始错误；既有包事务负责自己的四对象暂存清理，其他写入者留下的文件不递归删除，若包已发布但导入暂存清理失败，错误明确说明包已经提交；编码器自身删除输出失败时也报告残留，进程中止或断电的残留不在返回错误清理保证之内

验证使用合成 44.1 kHz 双声道 WAV 检查真实 4800 帧输出、源 BLAKE3 / 元数据、最终 PCM 能量、手工谱面、已有包保护、非法 authoring 与损坏源的失败清理；软件导入行为不代表真实音乐听感或设备时序验收

本机固定 lab / game 副本另通过 10 条成功 CLI、5 条预期拒绝及两组实际生产 package loader / GPU 图；短源 4410 帧真实输出 4800 帧，64 秒原创输入输出 3072000 帧，完整 Stage 整数采样与 lab 一致。原始源身份误用和 sandbox GPU 失败保留，固定二进制、437 / 419 项构建输入及主线程目检记录见[持久观察](../testdata/synthetic/source-import-observations-20261007.json)，复现见[软件工具](../tools/source-import-check/README.md)

游戏入口在创建并完整验证四对象之后调用现有 package loader；loader 会拒绝未知规则，即使该手工包已经合法发布，也不能将运行失败解释为包未创建，原已发布对象仍保留
