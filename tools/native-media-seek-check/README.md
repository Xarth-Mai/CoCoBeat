# Native canonical seek 软件控制

此工具复用已有 `tools/canonical-audio-probe/seek.rs`，用实际 Cargo JSON 中的 Symphonia rlib 编译同一窗口 reader；输入是固定 shipping 编码器已经产生且通过严格完整回读的八个 Ogg，完整基线由固定 shipping driver 的 Symphonia 与 libvorbisfile 分别重新读取，不重新编码或播放

```sh
python tools/native-media-seek-check/check.py --self-check
python tools/native-media-seek-check/check.py \
  target/vorbis-admission-20261007/shipping-build/build.jsonl \
  target/vorbis-admission-20261007/shipping-build \
  target/vorbis-admission-20261007/portable-shipping \
  target/vorbis-admission-20261007/seek/new-observation
```

输出目录须不存在；`immutable-binaries.json` 固定 driver 身份，Ogg 须匹配对应 shipping `result.json` 的成功状态、真实 N 和 SHA256，记录实际 rustc、Cargo artifact、helper / driver / 依赖 rlib 的前后身份与命令日志

每个输入包含头尾、Vorbis overlap 附近、重复交错定位和 64 个固定种子的随机目标，同一个 reader 连续执行各目标；4096 帧窗口必须匹配完整基线的绝对帧切片、真实剩余帧数、finite 和 `1e-6` 绝对容差，不对齐、拟合增益、补零或丢弃不匹配样本

原始 `Accurate` 与显式前滚分别记录，原始失败保持 FAIL；前滚仅覆盖本次逐文件识别出的 canonical q10 最大块 2048，1024 是 overlap 前滚长度，实际 observed delay 是独立字段；目标不大于 1024 时重新打开并从头读，其余先定位到目标减 1024、reset decoder，再丢弃有明确时间戳的前滚样本；这是正常窗口定位，不修剪完整流或制造样本

`target=N` 是合法空窗口，负数与 `N+1` 须明确拒绝；完整 baseline 在身份和比较记录后删除，窗口 PCM 与原始日志保留；超时、异常和失败不转为通过

2026-10-07：8 个输入共 695 个原始窗口中 127 个 API 失败，695 个前滚窗口全部通过，32 个非法目标拒绝通过；前滚与完整 Symphonia 逐样本完全一致，对 libvorbisfile 最大差 `1.1920928955078125e-7`，原始失败包括单帧输入、尾部和 EOF，见 [持久观察记录](../../testdata/synthetic/native-vorbis-seek-20261007.json)

结论限定为本机软件控制，不是 Symphonia 的修复或任意合法 Vorbis 的 seek 保证；游戏目前完整解码共享 PCM，Kira 的 PCM 游标不依赖此 demux seek，未来流式接线需单独验证；四平台 seek、真人听感和设备时序为 NOT RUN，FFmpeg 未参与本次 seek 控制
