# 源音频导入回归样本

三个原件均为本项目使用 FFmpeg 的正弦发生器生成的原创合成音频，作者为 CoCoBeat contributors，音频数据以 CC0-1.0 提供，生成命令与测试代码遵循仓库 MPL-2.0

`mono.mp3` 是 44,100 Hz、单声道、220 Hz、0.2 秒音频，使用 libmp3lame VBR quality 2 和 Xing/LAME gapless 信息，源音频为 8,820 帧；`mono.ogg` 是 48,000 Hz、单声道、220 Hz、2.1 秒音频，使用 libvorbis quality 2，源音频为 100,800 帧，包含三个音频页

`stereo-canonical.ogg` 是 48,000 Hz、0.1 秒双声道音频，左声道为峰值 0.1 的 440 Hz 正弦，右声道为峰值 0.2 的 880 Hz 正弦，使用 libvorbis quality 5，源音频为 4,800 帧；用于核对严格 canonical 读回的实际帧数、左右声道与消费方取消，不据此准入产品编码器或证明完整音质

FFmpeg 和原生编码器仅用于开发期生成独立样本，不进入产品依赖或测试执行环境；测试载入原件，在内存中改写 MP3 长度、Vorbis 采样率和 Ogg EOS 声明，以及损坏 CRC 和删除数据，以检查合法 padding、mono 复制、严格读回合同和坏包时间线拒绝；有效采样率/EOS 变体重算 CRC，避免由无关校验提前掩盖对应行为

生成环境为 FFmpeg n9.0.2，精确命令如下，从仓库根目录执行；不同编码器版本可能产生不同字节，回读帧数与时轴行为应单独核验

```sh
ffmpeg -hide_banner -loglevel error -nostdin -f lavfi -i 'sine=frequency=220:sample_rate=44100:duration=0.2' -map_metadata -1 -c:a libmp3lame -q:a 2 -write_xing 1 -id3v2_version 0 -fflags +bitexact -flags:a +bitexact -y testdata/synthetic/media-import/mono.mp3
ffmpeg -hide_banner -loglevel error -nostdin -f lavfi -i 'sine=frequency=220:sample_rate=48000:duration=2.1' -map_metadata -1 -c:a libvorbis -q:a 2 -fflags +bitexact -flags:a +bitexact -serial_offset 0 -y testdata/synthetic/media-import/mono.ogg
ffmpeg -hide_banner -loglevel error -nostdin -f lavfi -i 'aevalsrc=0.1*sin(2*PI*440*t)|0.2*sin(2*PI*880*t):s=48000:d=0.1' -map_metadata -1 -c:a libvorbis -q:a 5 -fflags +bitexact -flags:a +bitexact -serial_offset 0 -n testdata/synthetic/media-import/stereo-canonical.ogg
cargo test --locked -p cocobeat-media codec_padding_and_ogg_gaps_preserve_or_reject_the_original_timeline
cargo test --locked -p cocobeat-media canonical_readback
```

| 文件 | 字节数 | SHA-256 |
| --- | ---: | --- |
| `mono.mp3` | 1822 | `d923f64fa8d33576b85608ac20f13a1cf512f8dfd0e94e9c0e1342d652ca6085` |
| `mono.ogg` | 7156 | `2e98fad0c74320af0e9a98ec6e3e3ddc9b219afdc90826abc78aa9bd625fe33a` |
| `stereo-canonical.ogg` | 5245 | `fad4556061e4253ae09c030fdc1e4f4f5a91de14227d147f15628d4e610c54cf` |

2026-10-03 独立包验证发现短 `stereo-canonical.ogg` 的默认 FFmpeg 解码仅输出前 4672 帧，Symphonia 连续输出源长度 4800 帧；共同部分差值小于 `7.46e-8`，显式关闭自动 trim 后的原始读回与原始正弦诊断确认完整尾部存在。默认 FFmpeg 跨解码器帧数结果保留为 FAIL，不据此裁剪产品时轴或修改此 fixture，详细边界见 [SongPackage 验证](../../../docs/testing.md#初始-songpackage-与手工创作入口)
