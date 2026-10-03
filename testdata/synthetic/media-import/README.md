# 源音频导入回归样本

两个原件均为本项目使用 FFmpeg 的正弦发生器生成的原创合成音频，作者为 CoCoBeat contributors，音频数据以 CC0-1.0 提供，生成命令与测试代码遵循仓库 MPL-2.0

`mono.mp3` 是 44,100 Hz、单声道、220 Hz、0.2 秒音频，使用 libmp3lame VBR quality 2 和 Xing/LAME gapless 信息，源音频为 8,820 帧；`mono.ogg` 是 48,000 Hz、单声道、220 Hz、2.1 秒音频，使用 libvorbis quality 2，源音频为 100,800 帧，包含三个音频页

FFmpeg 和原生编码器仅用于开发期生成独立样本，不进入产品依赖或测试执行环境；测试只载入这两个原件，在内存中改写 MP3 长度声明、损坏 Ogg 中间页 CRC 和删除中间页，以检查合法 padding、mono 复制和坏包时间线拒绝

生成环境为 FFmpeg n9.0.2，精确命令如下，从仓库根目录执行；不同编码器版本可能产生不同字节，回读帧数与时轴行为应单独核验

```sh
ffmpeg -hide_banner -loglevel error -nostdin -f lavfi -i 'sine=frequency=220:sample_rate=44100:duration=0.2' -map_metadata -1 -c:a libmp3lame -q:a 2 -write_xing 1 -id3v2_version 0 -fflags +bitexact -flags:a +bitexact -y testdata/synthetic/media-import/mono.mp3
ffmpeg -hide_banner -loglevel error -nostdin -f lavfi -i 'sine=frequency=220:sample_rate=48000:duration=2.1' -map_metadata -1 -c:a libvorbis -q:a 2 -fflags +bitexact -flags:a +bitexact -serial_offset 0 -y testdata/synthetic/media-import/mono.ogg
cargo test --locked -p cocobeat-media codec_padding_and_ogg_gaps_preserve_or_reject_the_original_timeline
```

| 文件 | 字节数 | SHA-256 |
| --- | ---: | --- |
| `mono.mp3` | 1822 | `d923f64fa8d33576b85608ac20f13a1cf512f8dfd0e94e9c0e1342d652ca6085` |
| `mono.ogg` | 7156 | `2e98fad0c74320af0e9a98ec6e3e3ddc9b219afdc90826abc78aa9bd625fe33a` |
