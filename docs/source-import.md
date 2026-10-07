# 手工内容的生产源导入

`cocobeat-lab import-authored-package SOURCE_AUDIO AUTHORING_JSON NEW_PACKAGE` 将有界源音频经唯一的 `encode_canonical_audio` 路径转换成完整四对象包，随后可由既有 `--package NEW_PACKAGE` 加载；authoring 使用[四对象契约](song-package.md)规定的手工 Anchor / 段落，当前游戏规则为 `duo-watermark-v1`

```sh
cargo run --locked -p cocobeat-lab -- import-authored-package /path/to/source.wav /path/to/authoring.json /path/to/new-package
cargo run --locked -p cocobeat-lab -- verify-package /path/to/new-package
cargo run --locked -p cocobeat-game -- --package /path/to/new-package
```

Rust 入口为 lab 的 `package::import_authored(source, authoring_path, destination) -> Result<(), String>`，源格式和采样率由既有 Symphonia 源解码器检查，支持 WAV / PCM、FLAC、MP3、Ogg Vorbis 的单轨单声道或双声道输入，源长不超过十分钟，源文件不超过 512 MiB；输入须为普通文件，符号链接不作为此入口的源文件

工具先在目标的父目录创建本次独占暂存目录，以 8192 字节块复制源音频并计算实际快照的 BLAKE3，编码只读取这个快照；导入后修改原来源文件不影响已发布包，来源哈希描述实际复制的字节，不代表原文件在复制期间从未被修改

编码器返回实际源采样率、源帧数和最终 48 kHz 帧数，工具不要求用户估算 N，也不裁切、补帧、归一化或替换超出编码数值域的音频；严格最终读回通过后，既有 `build_package` 再复制并严格回读最终 Ogg，从这个确切副本测量能量，验证全部四对象后一次性发布新目录

`analysis.diagnostics` 保留来源快照的 BLAKE3、字节数、源采样率、源 N、canonical N、编码 profile 及作者 `source_note`，`manifest.importer_version` 记录 lab 版本和唯一 encoder profile；实际最终 Ogg 的长度与 BLAKE3 继续由 manifest 的音频对象引用记录，包 hash 绑定这些元数据和谱面身份

Anchor / 段落坐标是最终音频的 48 kHz 整数帧，工具不将源采样率坐标自动换算成谱面；越界、重复或倒序内容由现有 schema 校验拒绝；beat / onset 检测没有运行，段落置信度保留 `None`，能量是真实最终 PCM 的 1024 帧 RMS / peak，这个入口不代表完整自动 MIR 或曲库界面已交付

已有目标目录、文件和符号链接均拒绝，失败尝试删除本次明确创建的源快照、编码输出和空暂存目录，并保留原始错误；既有包事务负责自己的四对象暂存清理，其他写入者留下的文件不递归删除，若包已发布但导入暂存清理失败，错误明确说明包已经提交；编码器自身删除输出失败时也报告残留，进程中止或断电的残留不在返回错误清理保证之内

验证使用合成 44.1 kHz 双声道 WAV 检查真实 4800 帧输出、源 BLAKE3 / 元数据、最终 PCM 能量、手工谱面、已有包保护、非法 authoring 与损坏源的失败清理；软件导入行为不代表真实音乐听感或设备时序验收

本机固定 lab / game 副本另通过 10 条成功 CLI、5 条预期拒绝及两组实际生产 package loader / GPU 图；短源 4410 帧真实输出 4800 帧，64 秒原创输入输出 3072000 帧，完整 Stage 整数采样与 lab 一致。原始源身份误用和 sandbox GPU 失败保留，固定二进制、437 / 419 项构建输入及主线程目检记录见[持久观察](../testdata/synthetic/source-import-observations-20261007.json)，复现见[软件工具](../tools/source-import-check/README.md)
