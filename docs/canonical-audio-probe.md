# Canonical 音频候选实验

2026-10-02 的阶段 05 前置实验已得到两个有限范围可继续评估的方案：OxideAV 的原生 Ogg 封装通过本次合成输入检查；rusty_vorbis 需要显式 priming 与 mux 适配后才通过，原始公开 API 的失败结果保持独立

未创建 media crate，未改变产品依赖或运行时音频路径；正式 media、Windows、seek、重采样、真实音乐与真人听感均为 NOT RUN

## 方法与证据边界

输入统一为 48 kHz、双声道 F32；左声道为峰值 0.125 的 440 Hz 正弦，右声道最后 min(128,N) 帧按绝对帧号交替 +0.75/-0.75，之前为零；保留各原始生成器的浮点计算次序，OxiAudio 使用 f32 相位，另两个候选使用 f64 相位后转 f32

Symphonia 0.6.1 的 `AudioDecoderOptions::default()` 启用 `gapless`，回读遵循 Ogg packet 的首尾裁剪信息；独立 FFmpeg 9.0.2 直接输出 F32，不重采样、不改声道、不归一化，不以 Ogg 声明时长代替实际解码帧数

Nyquist 交替尾信号的衰减不能单独判定失败；另用 1 kHz、sin² 包络的首尾各 256 帧脉冲观察位置与能量，OxideAV 将峰值归一化到 0.5，rusty 适配器保留原峰值系数 0.5，两个实验的信号与质量刻度均不视为完全等价

[固定观察清单](../testdata/synthetic/canonical-audio-probe/observations.json) 保存版本、发布 archive SHA-256、参数、主要数值与原始结果/源码哈希；[原创实验源码](../tools/canonical-audio-probe/) 保留三个独立 manifest 与原始 lock，各包通过空 `[workspace]` 与产品工作区隔离，共用一个 Symphonia reader，不引入 helper crate

## OxiAudio Encode 0.2.1：FAIL

[发布接口](https://docs.rs/oxiaudio-encode/0.2.1/oxiaudio_encode/) 可产出两解码器均接受的文件，但实际帧数与幅度错误

| 输入帧数 | EOS granule | 两个解码器实际帧数 | 观察 |
| ---: | ---: | ---: | --- |
| 48000 | 47104 | 46080 | 少 1920 帧，即 40 ms |
| 48128 | 48128 | 47104 | 少 1024 帧 |
| 1024 | 1024 | 0 | 非空输入回读为空 |
| 0 | 1024 | 0 | 空输入写入非零结束 granule |

48000 帧输入回读左峰值 21608716、右峰值约 36476，远超原幅度；q0/q5/q10 输出逐字节相同，Symphonia 统计样本均有限；有限值和解码退出码为 0 均不能抵消上述失败

该版本源码将编码帧数按 1024 取整，质量参数主要影响静默阈值，残差量化保持固定；这些结论只针对已测试发布版本与入口

## oxideav-vorbis 0.0.12：PASS_STRUCTURAL

[原生 `encode_pcm_to_ogg`](https://docs.rs/oxideav-vorbis/0.0.12/oxideav_vorbis/) 在空输入时明确拒绝；其余九例覆盖 1/1024/1025/48000/48128 帧、三个质量值、首尾脉冲和 64 秒输入，两路实际帧数与 EOS 均精确等于输入，格式为 48 kHz/stereo，样本全部有限，Ogg 页 CRC 全部通过

`PASS_STRUCTURAL` 检查格式、帧数、有限值及成功解码；下表幅度和脉冲指标是本次输入的观测，不是普遍音质准入阈值

| 48000 帧、交替尾输入 | Ogg 字节 | FFmpeg 左峰值 | FFmpeg 右峰值 |
| --- | ---: | ---: | ---: |
| quality=0 | 3431 | 0.144488 | 0.820295 |
| quality=0.5 | 5780 | 0.126488 | 0.757296 |
| quality=1 | 9078 | 0.125217 | 0.750887 |

三个输出哈希不同；这是本库 `[0,1]` 配置，不与 libvorbis q5 或 rusty_vorbis 刻度等同，输出不同也不单独证明音质改善

脉冲头/尾能量分别为输入的 1.000585/1.000365，质心偏移约 -0.0550/-0.0945 帧，峰值位置仍为 132/47876；两解码器逐点差异 RMS 均小于 1e-6

锁定编码器依赖闭包的 17 个 crate archive 校验值与 Cargo.lock 一致，823 个本地源文件与发布归档一致，未发现原生 codec C 绑定或 `links` 元数据；这不表示 Linux 二进制不链接 libc，摘要保存原审查结果哈希，不复制第三方源码或一次性全包扫描器

## rusty_vorbis 0.1.1：原始 API FAIL，显式适配 PASS_LIMITED

[官方高层 API](https://docs.rs/rusty_vorbis/0.1.1/rusty_vorbis/) 的原始实验保持 `EncodedPacket.pts` 不变，经 ogg 0.8.0 每包一页封装，布局跟随发布包的测试 mux；1/1024/1025 帧均回读 0 帧，48000 帧回读 46080 帧，48128 帧回读 47104 帧，两独立解码器结果一致

原实现从输入偏移 0 开始取 2048 帧窗口，步长 1024；首个 overlap 包也累加 pts，短输入不足以产生可回读窗口，因此只修改最终 granule 无法补回缺失音频

独立的 `adapter` 入口向公开编码 API 前后各附加 1024 帧双声道零；保存的真实输入仍为原始 N 帧，保留前三头及 `ceil(N/1024)+1` 个音频包，首音频 granule=0，后续为 `min(i*1024,N)`，最后 EOS=N，其余 padding 包丢弃；packet 字节不改写，原始 pts、适配 granule 与保留标记逐包记录

| 原始输入帧数 | 适配后两路帧数及 EOS | Symphonia 左 RMS 相对输入 |
| ---: | ---: | ---: |
| 1 | 1 | 不作稳态幅度结论 |
| 1024 | 1024 | -0.567 dB |
| 1025 | 1025 | +0.003 dB |
| 48000 | 48000 | -0.230 dB |
| 48128 | 48128 | -0.232 dB |

适配实验 q5 的短样本、脉冲与 64 秒输入均符合本次门槛：精确帧数、48 kHz/stereo、样本有限、峰值≤1，以及 N≥1024 时左侧稳态 RMS 差在±3 dB 内；空输入明确拒绝，这些门槛只用于当前合成信号

脉冲头/尾最佳互相关时移为 +1/0 帧，相关值约 0.96165/0.97619，512 帧边界窗口能量变化 -1.522/-0.865 dB，能量质心变化 -5.047/-0.720 帧；通过本次探索性门槛：能量±3 dB、时移±2帧、相关≥0.95、质心偏移±8帧，未观察到残留 1024 帧系统偏移

原始 API 的 q0/q5/q10 输出不同，适配版本仅测试 q5，适配版本其它质量值仍为 NOT RUN；编码器为零依赖 Rust 实现，默认 SIMD 含 Rust `std::arch` unsafe AVX2，不能称为全 safe Rust；PCM 与编码包仍全曲缓存，适配不提供有界流式内存

## 64 秒单次资源观察

| 候选 | 实际双路帧数 / EOS | Ogg 字节 | 墙钟秒 | 最大 RSS KiB |
| --- | ---: | ---: | ---: | ---: |
| OxideAV quality=0.5 | 3072000 | 189899 | 206.297571 | 392520 |
| rusty 显式适配 q5 | 3072000 | 244327 | 0.164765 | 31784 |

两例样本全部有限；OxideAV 使用 Linux `wait4` 统计 timeout 进程树，RSS 包含短暂继承的 Python 内存，64 秒编码设 600 秒与 2 GiB 虚拟地址空间上限；rusty 使用新 Python long 进程的首个编码子进程 `resource.RUSAGE_CHILDREN`，含输入生成/写入、padding、编码和 mux，不含编译与解码，user/system CPU 为 0.953083/0.019955 秒

环境为 Ryzen 7 5700X、16 个可用逻辑 CPU；每候选只有一次该合成输入测量，算法和质量配置不同，可能存在并行编码或构建，未进行受控吞吐排名，也未验证长于 64 秒的资源上限

## 从 fresh clone 复现

需要 Rust/Cargo、Python 3.11+、FFmpeg/ffprobe；OxideAV 的资源限制脚本另需 Linux 与 GNU timeout，本次 Rust/Cargo 为 1.98.1、FFmpeg 为 n9.0.2，精确 crate 版本由各自 Cargo.lock 固定；首次获取依赖需要网络，已有缓存时可给构建加 `--offline`

以下命令从仓库根目录运行，所有生成物写入忽略的 `target/canonical-audio-probe/`；输出目录和编译二进制目录都是显式参数，可替换为其它位置，重复运行会更新该输出目录，原始历史观察清单保持不变

```sh
probe_root=tools/canonical-audio-probe
probe_output=target/canonical-audio-probe
probe_build=target/canonical-audio-probe/build
for candidate in oxiaudio oxideav rusty-vorbis; do
  cargo build --locked --release --manifest-path "$probe_root/$candidate/Cargo.toml" --target-dir "$probe_build/$candidate"
done
python3 -B "$probe_root/oxideav/test_metrics.py"

"$probe_build/oxiaudio/release/cocobeat-canonical-probe" "$probe_output/oxiaudio"
python3 -B "$probe_root/oxiaudio/analyze.py" "$probe_output/oxiaudio" "$probe_build/oxiaudio/release"

python3 -B "$probe_root/oxideav/probe.py" "$probe_output/oxideav" "$probe_build/oxideav/release"
python3 -B "$probe_root/oxideav/probe.py" "$probe_output/oxideav" "$probe_build/oxideav/release" --single 48000 --quality 0.5 --signal burst
python3 -B "$probe_root/oxideav/probe.py" "$probe_output/oxideav" "$probe_build/oxideav/release" --single 3072000 --quality 0.5 --encode-seconds 600
python3 -B "$probe_root/oxideav/summarize.py" "$probe_output/oxideav"

python3 -B "$probe_root/rusty-vorbis/run.py" "$probe_output/rusty-vorbis" "$probe_build/rusty-vorbis/release"
python3 -B "$probe_root/rusty-vorbis/run_adapted.py" "$probe_output/rusty-vorbis" "$probe_build/rusty-vorbis/release" short
python3 -B "$probe_root/rusty-vorbis/run_adapted.py" "$probe_output/rusty-vorbis" "$probe_build/rusty-vorbis/release" pulse
python3 -B "$probe_root/rusty-vorbis/run_adapted.py" "$probe_output/rusty-vorbis" "$probe_build/rusty-vorbis/release" long
```

OxiAudio 的结果 JSON 与 rusty baseline 的 `FAIL` 是预期失败证据，脚本退出 0 表示完成测量，不代表候选合格；OxideAV 的状态位于各结果 `status/checks`，rusty 适配结果位于 `adapted-results.json`，原始与适配结果分目录保存

共享 reader 也可独立执行 `readback INPUT_OGG OUTPUT_F32LE`，输出格式和实际帧数 JSON；候选编码入口分别接受 `OUTPUT_DIR`、`OUTPUT_DIR FRAMES QUALITY [tail|burst]`、`OUTPUT_DIR FRAMES Q` 与适配器的 `OUTPUT_DIR FRAMES Q [pulse]`

## 本次持久化验证

三个工具包在独立 `/tmp` 构建目录以 `--offline --locked --release -j 2` 构建通过；重新运行 OxiAudio 四种边界、OxideAV 1025 帧、rusty baseline 八例及适配器短样本五例和脉冲，共享 reader 与 FFmpeg 保持各自原 PASS/FAIL 帧数结果

三个 lock 与原文件逐字节相同；OxiAudio 四例原始输入及所有质量输出、OxideAV 1025 帧、rusty baseline/adapter 1025 帧与适配脉冲的输入、Ogg、两路回读 PCM 逐字节匹配原证据；没有因路径迁移重跑 206 秒长实验，历史 64 秒结果与源码身份由观察清单保存

分析器的 NaN/±Inf 检查已修复：保留 `nonfinite` 失败计数，把无效 RMSE/SNR 与预览值写为 JSON null，避免统计结果在落盘时丢失；一个标准库自检覆盖有限值不变和三种非有限值，历史全有限数值不变

原始全量材料仍在本地 `target/canonical-probe/`、`target/oxideav-probe/`、`target/rusty-vorbis-probe/`；PCM/Ogg、第三方源码、crate archive、编译缓存和大日志不进入 Git，仓库保留原创生成/分析代码、锁文件与小型固定观察清单
