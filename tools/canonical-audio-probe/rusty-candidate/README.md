# rusty_vorbis q10 独立候选

包名与 binary 均为 `cocobeat-rusty-candidate`，入口复用项目 `cocobeat-media`：源文件 → 完整解码和48 kHz立体声重采样 → patched q10编码 → Ogg封装 → 项目解码器完整回读最终Ogg

```sh
cargo test --locked --manifest-path tools/canonical-audio-probe/rusty-candidate/Cargo.toml
cargo build --release --locked --manifest-path tools/canonical-audio-probe/rusty-candidate/Cargo.toml
tools/canonical-audio-probe/rusty-candidate/target/release/cocobeat-rusty-candidate source.wav new-output-dir
```

可用 `--target <triple> --target-dir <dir>` 指定原生验证目标与构建目录，最后运行相应路径的 binary；工具不依赖 FFmpeg、Bevy、音频设备或外部测试资源，测试用 std 生成短 WAV

## 输出与失败约定

`<new-output-dir>` 的父目录须已存在，工具原子创建新目录并拒绝现有路径；目录内文件均以 `create_new` 创建，失败退出码为1，参数数量错误为2

| 文件 | 内容 |
| --- | --- |
| `resampled.f32le` | 实际送入编码器的48 kHz交错立体声 F32 LE，不含前后 priming |
| `canonical.ogg` | 固定 profile 的 patched q10 Vorbis 候选 |
| `decoded.f32le` | 项目解码器从最终 Ogg 完整回读的交错立体声 F32 LE |
| `report.json` | 源字节 SHA256、输入/输出精确帧数、峰值、overs、SNR、三份产物 SHA256、主机与编码参数 |

只有源文件处理、PCM写入、编码器 drain、Ogg封装、最终完整回读和报告写入全部成功，进程才返回0；成功报告状态为 `PASS_SOFTWARE_CANDIDATE`，失败报告为 `FAIL` 并记录错误，失败时保留新目录中的部分产物用于诊断，不把这些部分产物视作有效候选

报告先完整写入并 flush 到 `report.pending.json`，关闭后重命名为 `report.json`；报告自身写入失败也返回1，残留的 pending 文件不是完成报告，既有输出目录不会创建或修改报告

`overs` 是各声道绝对值大于1的样本数；解码后的有限 overshoot 被记录而不裁剪；`snr_db` 在输入能量为0或误差能量为0时为 null，配套能量字段可区分两种情况；SNR不设置事后音质门槛，软件 PASS 不代表真人听感、音乐分析准入或 SongPackage Ready

## 固定边界

- 源文件格式、采样率、声道和长度约束沿用项目 media；工具检查重采样实际帧数，范围为1–28,800,000帧，即最多600秒
- upstream `push_pcm_f32` 文档约定 `[-1,1]`，候选在重采样后显式检查该范围；media允许有限超范围PCM，重采样也可能 overshoot，这些输入对本候选返回 `unsupported input domain`，不归一化、不裁剪，不将其误判为非法源媒体
- q10映射为 upstream 的0.98质量值，编码器会缓存全曲并在 drain 时编码全部块，不是 streaming encoder；600秒上限不是运行时内存限额承诺，资源门槛需外部进程限额验证
- 沿用已验证 adapter：前后各1024帧零填充，2048长块、1024 hop、保留 `3 + ceil(N/1024) + 1` 个包，音频包 granule 为 `min((index-3)*1024,N)`，最后一个保留包标记 EOS
- 每包检查当前固定 profile：48 kHz立体声、256/2048声明、音频 mode1与前后long flags；若上游产生其他模式立即失败，不能直接把这套时序公式推广到动态短块编码器
- 项目解码器完整回读后的采样率必须是48 kHz，media计数与回调实际计数必须都等于重采样帧数，全部PCM必须有限；源文件处理前后的 SHA256 必须相同

官方副本、Apache许可、逐文件身份与唯一函数补丁见 [UPSTREAM.md](UPSTREAM.md)、[upstream.json](upstream.json) 和 [patches/max-abs-coupling.patch](patches/max-abs-coupling.patch)；manifest只在本独立工具引用 patched vendor，现有 published probe 及产品编码器未切换
