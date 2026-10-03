# Canonical 音频候选实验

2026-10-03 的阶段 05 实验已覆盖长曲、信号边界、精确 seek 和重采样合成质量；OxiMedia High 通过本批频点门槛，OxideAV 在十分钟输入、2 GiB 虚拟地址空间限制下编码失败，rusty_vorbis 仍因近满幅合法输入的明显波形误差不准入；以下保留首轮结构结果与后续发现

`cocobeat-media` 的有界源解码已开始实现；生产 canonical Vorbis 编码尚未准入，现有运行时音频路径未接入候选编码器；已测一首原创开发歌曲，Windows、外部音乐曲库与真人听感仍为 NOT RUN，源解码进展不改变本文候选实验的准入边界

## 方法与证据边界

输入统一为 48 kHz、双声道 F32；左声道为峰值 0.125 的 440 Hz 正弦，右声道最后 min(128,N) 帧按绝对帧号交替 +0.75/-0.75，之前为零；保留各原始生成器的浮点计算次序，OxiAudio 使用 f32 相位，另两个候选使用 f64 相位后转 f32

Symphonia 0.6.1 的 `AudioDecoderOptions::default()` 启用 `gapless`，回读遵循 Ogg packet 的首尾裁剪信息；独立 FFmpeg 9.0.2 直接输出 F32，不重采样、不改声道、不归一化，不以 Ogg 声明时长代替实际解码帧数

Nyquist 交替尾信号的衰减不能单独判定失败；另用 1 kHz、sin² 包络的首尾各 256 帧脉冲观察位置与能量，OxideAV 将峰值归一化到 0.5，rusty 适配器保留原峰值系数 0.5，两个实验的信号与质量刻度均不视为完全等价

[首轮固定观察清单](../testdata/synthetic/canonical-audio-probe/observations.json) 保存版本、发布 archive SHA-256、参数、主要数值与原始结果/源码哈希；[原创实验源码](../tools/canonical-audio-probe/) 保留三个编码候选和一个重采样候选的独立 manifest 与 lock，各包通过空 `[workspace]` 与产品工作区隔离，编码候选共用一个 Symphonia reader，不引入 helper crate

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

## rusty_vorbis 0.1.1：原始 API FAIL，首轮适配 PASS_LIMITED

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

原始 API 的 q0/q5/q10 输出不同，首轮适配版本仅测试 q5，后续 q10 诊断见下文；编码器为零依赖 Rust 实现，默认 SIMD 含 Rust `std::arch` unsafe AVX2，不能称为全 safe Rust；PCM 与编码包仍全曲缓存，适配不提供有界流式内存

## 64 秒单次资源观察

| 候选 | 实际双路帧数 / EOS | Ogg 字节 | 墙钟秒 | 最大 RSS KiB |
| --- | ---: | ---: | ---: | ---: |
| OxideAV quality=0.5 | 3072000 | 189899 | 206.297571 | 392520 |
| rusty 显式适配 q5 | 3072000 | 244327 | 0.164765 | 31784 |

两例样本全部有限；OxideAV 使用 Linux `wait4` 统计 timeout 进程树，RSS 包含短暂继承的 Python 内存，64 秒编码设 600 秒与 2 GiB 虚拟地址空间上限；rusty 使用新 Python long 进程的首个编码子进程 `resource.RUSAGE_CHILDREN`，含输入生成/写入、padding、编码和 mux，不含编译与解码，user/system CPU 为 0.953083/0.019955 秒

环境为 Ryzen 7 5700X、16 个可用逻辑 CPU；每候选只有一次该合成输入测量，算法和质量配置不同，可能存在并行编码或构建，未进行受控吞吐排名；首轮止于 64 秒，后续 rusty 十分钟检查见下文

## 后续长曲与近满幅检查

[后续固定观察](../testdata/synthetic/canonical-audio-probe/followup-observations.json) 记录本节数值、来源文件哈希与源码身份，首轮观察不改写

rusty q5 适配器的 600 秒输入、两路完整解码与 EOS 均为 28,800,000 帧；编码子进程墙钟 1.4942 秒、最大 RSS 234252 KiB，包含输入生成/写入、padding、编码与 mux，限制为 600 秒和每进程 2 GiB 虚拟地址空间；28,800,001 帧在生成 PCM/Ogg 前明确拒绝；并行 release 构建期间测得的单次数据不作性能排名

全静默回读精确为零；首尾各 100 ms 静默的三秒样本保留全部帧，靠近声音边缘有非零泄漏，右前静默峰值约 0.11198，避开边缘 2048 帧后静默精确为零；这些是波形观察，不将静默区有泄漏等同于时间被裁掉

近满幅输入为一秒、48 kHz 双声道 440/1000 Hz 正弦，峰值 0.99；四次编码使用逐字节相同的 PCM，未裁剪、归一化或错位对齐；下表为 Symphonia 完整回读的左/右指标，FFmpeg 结果在浮点误差内一致

| 编码入口 | 输出峰值 | 对原输入的 SNR dB | 超过 1 的样本数 / 每声道 48000 |
| --- | --- | --- | --- |
| rusty 适配 q5 | 1.3711 / 1.9372 | 18.641 / -1.289 | 4248 / 7299 |
| rusty 适配 q10 | 1.0709 / 1.4648 | 29.483 / -0.993 | 1708 / 7597 |
| libvorbis q5 实验参照 | 1.0132 / 1.0136 | 38.910 / 40.018 | 1030 / 581 |
| OxideAV quality=0.5 | 0.9973 / 0.9928 | 48.357 / 47.981 | 0 / 0 |

SNR 使用同一绝对帧位置的输入能量与误差能量；排除首尾各 2048 帧后，rusty 右声道 q5/q10 仍为 -1.516/-1.250 dB，libvorbis 为 40.644 dB，因此不能只用文件边缘的峰值解释；本次将 rusty 标为 `FAIL_QUALITY_CURRENT_CASE`，提高到最高官方 q10 也未解决，结构 `PASS_STRUCTURAL` 仍单独保留；不同库质量刻度和码率不等价，这个样本不支持普遍音质排名

libvorbis 仅通过已安装 FFmpeg 执行独立参照，不进入产品或研究 Rust 依赖；当前不以它替换唯一纯 Rust 编码路径，OxideAV 的完整音质、十分钟资源和 Windows 门槛仍未通过

### rusty 独立 coupling 诊断：根因确认，未准入

冻结的 20 个短例覆盖发布版/独立补丁 × q5/q10 × 近满幅、低幅度、交换左右频率、相同声道、反相声道；每例为 48 kHz 双声道 48000 帧，同位置比较不调整时间或增益，两路回读帧数精确、样本全部有限，最大逐点解码差为 4.76837e−7；发布版近满幅 q5/q10 的输入、Ogg 和两路 PCM 重现之前的原字节结果

当前严重右声道失真的根因位于 `frame.rs::forward_couple`：异号残差时可能将较小的左残差选为 magnitude，量化到零后，逆 coupling 切换分支并翻转较大的右残差符号；真实 q10 VQ 记录中，L=0.006012407、R=−6.0681543 被编码为 M=0.006012407、A=6.074167，重建 M=0、A=6 后输出 R=+6；本批记录到 58 次大于 1 的右残差符号翻转，补丁后为 0，诊断日志未改变各自 Ogg 字节

独立 `max-abs-coupling.patch` 仅修改该函数，选绝对值较大的残差作为 magnitude，再按其符号计算 angle；这是未发行的开发补丁，修复了本例根因，不替换官方 0.1.1 的历史 FAIL，也不构成生产准入；下表为近满幅输入的补丁后左/右观测，原版指标保留在上表

| 补丁入口 | 全段 SNR dB | 内部 SNR dB | 输出峰值 | 超过 1 的样本数 / 每声道 48000 |
| --- | --- | --- | --- | --- |
| q5 | 18.824 / 16.464 | 18.893 / 16.862 | 1.15957 / 1.77993 | 4117 / 3482 |
| q10 | 29.092 / 30.880 | 28.965 / 30.962 | 1.07511 / 1.05242 | 1755 / 2196 |

q5 仍有明显波形误差与 overs，不能仅因右声道改善便准入；有损编码的 overs 本身也不等于结构失败。相同声道的两档 Ogg 原字节不变，但反相声道 q10 的左/右 SNR 从 30.069/24.999 变为 24.866/30.043 dB，双声道总 SNR 从 26.8327 小幅降为 26.7251 dB，补丁并未改善所有输入或每个声道

[固定观察清单](../testdata/synthetic/canonical-audio-probe/admission-observations-20261003.json) 归档全部 20 例的紧凑指标、命令、源文件/补丁/二进制及报告哈希；本批重新核对 19 项冻结身份与 80 个输入/Ogg/PCM 哈希，均匹配；本批不覆盖该补丁的 64 秒或十分钟实验、seek、Windows/ARM 运行和真人听感，后续长曲结果另行归档

## 精确 seek：原生 FAIL，显式前滚 PASS_LIMITED

用两个候选各自的 64 秒文件与一秒首尾脉冲，反复前后 seek 25 次，窗口最多 4096 帧；原生 `Accurate` 调用每文件 9 PASS / 16 FAIL；额外请求前滚 1024 帧后，四文件的 100 个正例与 8 个拒绝负例全部通过，窗口与完整 Symphonia PCM 最大差为 0，与 FFmpeg 最大差不超过 1.1921e-7

本批输入均为 `start_ts=-1024`、`delay=1024`，可播放首帧仍为 PTS 0；Symphonia 0.6.1 Ogg 将 EOS=N 存为 num_frames，却用 start_ts+num_frames 作 seek 上界，合法尾帧 N-1023..N-1 被拒；头部目标 1..1023 又会跳过有效长度为零的首预热包，decoder reset 后首可输出帧移到 1024；独立源码复核对应 `symphonia-format-ogg/src/demuxer.rs:170–173,271–277,507–515`、`logical.rs:500–506` 与 Vorbis decoder `lib.rs:317–329`，改用 Time 请求不解决同一问题

实验适配请求 `max(0,target-1024)`，seek 后 reset，同一 decoder 消费前滚后按 packet.pts+trim_start 裁到原始 target；保留 target/request/actual 各值，不补零、不修改 Ogg，也不替换解码器；N 是空输出 EOF 正边界，负值与大于 N 明确拒绝；这个固定前滚量只验证了本批 2048 最大块、1024 delay 文件，不能泛化为所有导入 Vorbis

## OxiMedia 重采样 API 预查

官方索引核实的 `oximedia-audio 0.2.1` 使用根导出 `Resampler`，关闭默认 codec features，显式 High、每块最多 1024 帧，最后追加一次 flush；六例 44.1/96 kHz 的空输入、单帧和一秒输入通过整数长度、48 kHz/stereo、有限值、重复 flush 为空及 flush 后拒绝输入检查

44.1 kHz 一秒实际输出 47896+104=48000 帧，但 `output_sample_count()` 的浮点上取整估为 48001；96 kHz 一秒为 47952+48=48000；时轴以累计实际帧数及最终编码回读为准，不能用这个容量估算 helper；单帧的独立整数预期分别为 2/1 帧，空输入为 0

源码有下采样 cutoff 缩放与真实窗化 sinc 卷积，首轮仅检查 API，后续合成质量见下节；API 的输入 sample_rate 需由调用方验证，输出块 timestamp 不能作连续时轴，`with_max_buffering` 仅为 advisory；同库 `resampler::SimpleResampler::Polyphase` 实际调用线性插值，与本次入口不同；[官方 API](https://docs.rs/oximedia-audio/0.2.1/oximedia_audio/resample/struct.Resampler.html)

## 2026-10-03：High 合成质量 PASS，十分钟编码 FAIL

[固定观察清单](../testdata/synthetic/canonical-audio-probe/admission-observations-20261003.json) 保存两项独立实验的数值、门槛、命令、源码与产物哈希；重采样成功不改变编码器准入结果

OxiMedia 0.2.1 High 输入为一秒 F32 双声道、峰值 0.5 的解析正弦，左右相位为 0/π÷4；分别测 44.1/48/96 kHz→48 kHz，每个输入以 1/1024/8192 帧分块独立处理，输出恰为 48000 帧且逐字节相同；另有 117 个空、单帧及块边界长度检查，使用整数 `ceil(N×48000/source_rate)`，全部通过

预设门槛为 1 kHz 对解析真值的 SNR≥70 dB、20 Hz/1 kHz/20 kHz 三个通带频点幅差≤±0.1 dB、96→48 kHz 的 26/30/40 kHz 阻带抑制≥60 dB；稳态统计去掉输出首尾各 4800 帧，幅相通过已知频率的正弦/余弦最小二乘拟合，真值 SNR 不调整增益或相位，另列拟合残差 SNR；阻带以完整稳态 RMS 对输入正弦 RMS 计算抑制，并测折叠频率的幅相

| 输入→输出 | 两声道最低 1 kHz 真值 SNR dB | 三个通带频点最大绝对幅差 dB |
| --- | ---: | ---: |
| 44.1→48 kHz | 138.109 | 0.000362416 |
| 48→48 kHz | 153.792 | 0.000000127638 |
| 96→48 kHz | 153.792 | 0.00000456049 |

96→48 kHz 的 26/30/40 kHz 输入分别折叠到 22/18/8 kHz，两声道最低抑制分别为 117.255/130.094/138.147 dB；三组全静默的六个声道全部精确为零，66 个流检查均通过有限值、长度、重复 flush 和分块一致性检查；同频 passthrough 没有 finished 状态，flush 后仍接受输入，该差异明确记录，仅实际重采样路径要求 flush 后拒绝输入

24/24.5/25/25.5 kHz 另列为过渡带观察；24 kHz 落在输出 Nyquist，拟合两基向量退化，幅相写为 null，左右相位导致抑制分别约 228.374/45.529 dB；其余三个频点两声道最低抑制约为 89.654/121.546/115.997 dB，均不用于扩展预设阻带门槛的适用范围

首尾脉冲分别放在左右声道的输入第 0/N−1 帧，保留完整输出，不作稳态裁剪；下表记录输出峰位置与能量质心相对解析位置的偏移，未设普遍边界音质准入阈值

| 输入→输出 | 首/尾峰位置偏移（帧） | 首/尾能量质心偏移（帧） |
| --- | --- | --- |
| 44.1→48 kHz | 0 / +0.088435 | +0.218780 / −0.177864 |
| 48→48 kHz | 0 / 0 | 0 / 0 |
| 96→48 kHz | 0 / −0.5 | +0.100721 / −0.740344 |

High 通过本批门槛，未继续测 Best；解析指标自检、fmt、Clippy 与 release 构建通过，原六例的 12 个 PCM 文件及 JSONL 逐字节不变；独立 Python DFT 复核 32 个声道频点，幅度计算差最大 6.57e−13 dB；这些结果不证明连续频带、长时内存、真实音乐、Windows 或真人听感合格

OxideAV 0.0.12 quality=0.5 的 600 秒、28,800,000 帧合成输入在每进程 2 GiB `RLIMIT_AS` 下运行 182.9655 秒后发生 `memory allocation of 4096 bytes failed`，编码进程退出 −6，`wait4` 记录最大 RSS 2,092,880 KiB；RSS 包含短暂继承的 harness 内存，不作吞吐排名，3600 秒超时上限未触发

该次未产生 Ogg，容器检查与双路完整回读均为 NOT RUN；28,800,001 帧的独立负例在创建输出前明确拒绝；保持 `FAIL_ENCODE` 和生产编码未准入，不通过扩大限制或缩短长曲门槛改写结果

发布版源码的 `oggfile.rs` 按全曲保留 `spectra`、`unpred`、`maskings`、`envelopes`、`targets` 与 `bin_weights` 等中间数组；`VorbisStreamEncoder` 也先累计全部 PCM，到 flush 才调用同一全曲编码流程，因此该入口不能视为有界流式编码；这是内存增长的源码依据，本次错误没有分配调用栈，未据此声称定位到具体失败分配

### 原创开发歌曲与受限 seek

同一发布版 quality=0.5 对项目原创开发歌曲的 64 秒、3,072,000 帧 F32 输入完成编码，耗时 383.287 秒、最大 RSS 469400 KiB、Ogg 1,367,881 字节；两路回读与 EOS 都为 3,072,000 帧，样本全部有限，Ogg 页与双路一致性检查通过，状态为 `PASS_STRUCTURAL`；Symphonia 左/右峰值为 0.297184/0.277194，超过 1 的样本为零，对原始同位置 PCM 的 SNR 为 33.861/33.110 dB，双解码器最大逐点差为 7.45058e−8；这些波形指标是单首歌曲观测，没有据此判定真人听感或覆盖十分钟失败

该文件 `delay=128`、`track_start=-128`；原生 seek 的 25 次窗口检查为 21 PASS / 4 FAIL，失败位于尾部及 EOF；旧前滚实验只允许 `delay=1024`，对新文件在执行任何窗口前由 guard 拒绝，应记录为实验适用范围不符，不能据此判定前滚算法失败

独立诊断探针保持原前滚、解码和同位置窗口比较逻辑，只将 guard 收窄到已核实的 Vorbis v0、48 kHz/stereo、256/2048 块及 delay=128/1024，仍额外请求 1024 帧前滚；原创歌曲、原 64 秒尾信号和一秒脉冲共 75 个窗口与 6 个越界拒绝负例通过，对两路完整 PCM 的最大差不超过 1.19209e−7；前滚为 0 时重现原 4 处失败，原始失败证据与共享探针源码保留，该结果仅适用于本组诊断 profile，未形成任意导入或生产 seek 准入

### 内存生命周期诊断

独立开发补丁尝试在两个阶段结束后释放整组旧矩阵；1025 帧和一秒脉冲的 8 个产物与发布版逐字节相同，但十分钟输入仍在相同 2 GiB 限制下分配失败，耗时 186.474 秒、最大 RSS 2,094,084 KiB、退出 −6，未生成 Ogg；记录为 `DEVELOPMENT_PATCH_NOT_RELEASED` 的 `FAIL_ENCODE`，不覆盖发布版结果

在两处释放补丁上追加阶段记录的诊断版本确认 `unpred` 已释放，随后进入 `targets` 构建阶段，尚未到达旧分析矩阵的第二个释放点便分配失败；日志记录 57,601,280 个声道频点，按源码元素类型估算，在新矩阵完整建立时仍存活的主要数组 payload 下限为 2,073,640,960 字节，尚未计入容器、容量余量、分配器与运行时开销；这定位了失败阶段和内存重叠，仍不是具体分配调用栈

逐行释放旧矩阵的独立补丁现已得到终态：1025 帧、一秒首尾脉冲和旧 64 秒合成尾信号共 12 个输入/Ogg/双路 PCM 文件与发布版逐字节相同；其中该补丁的 64 秒回归耗时 201.301 秒、最大 RSS 245344 KiB，这不是未修改上游版本的新基线，也不作吞吐排名

该逐行释放补丁的十分钟输入仍在每进程 2 GiB 虚拟地址空间限制下失败：519.248550638 秒、最大 RSS 2093400 KiB、退出 −6，报 `memory allocation of 6144 bytes failed`，3600 秒超时未触发；已完成 targets 行构建及旧分析矩阵释放，失败位于 `training_begin` 与 `final_plans_begin` 标记之间，标记覆盖训练数据构造、训练调用和训练后分配，不能定位具体迭代或分配调用栈

本次未产生 Ogg，容器与双路完整回读为 NOT RUN；28,800,001 帧负例仍在创建输出前拒绝，记录为 `DEVELOPMENT_ROW_LIFETIME_PATCH_NOT_RELEASED` 的 `FAIL_ENCODE`，发布版及之前两个开发补丁的失败记录保留，生产编码仍未准入

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

共享 reader 也可独立执行 `readback INPUT_OGG OUTPUT_F32LE`，输出格式和实际帧数 JSON；OxideAV 新增 `near-full` 信号，rusty 适配器新增 `silence / near-full / edge-silence`，原默认调用与旧字节保持不变

后续实验复用上面的三个目录变量，Linux 资源限制依赖 GNU timeout；以下命令运行十分钟矩阵、近满幅样本、一个 seek 矩阵和六例重采样 API 检查，原生 seek 将前滚设为 0 并换新输出目录即可复现 FAIL

```sh
python3 -B "$probe_root/rusty-vorbis/limits.py" --self-check
python3 -B "$probe_root/rusty-vorbis/limits.py" "$probe_output/rusty-limits" "$probe_build/rusty-vorbis/release"
python3 -B "$probe_root/oxideav/probe.py" "$probe_output/oxideav-near-full" "$probe_build/oxideav/release" --single 48000 --quality 0.5 --signal near-full

python3 -B "$probe_root/seek.py" --self-check
seek_case="$probe_output/oxideav/cases/n3072000-q0.5"
python3 -B "$probe_root/seek.py" "$probe_build/oxideav/release/seek" "$seek_case/encoded.ogg" "$seek_case/symphonia.f32le" "$seek_case/ffmpeg.f32le" "$probe_output/seek-oxideav-64s" --preroll-frames 1024

cargo build --locked -j 1 --manifest-path "$probe_root/oximedia-resample/Cargo.toml" --target-dir "$probe_build/oximedia-resample"
"$probe_build/oximedia-resample/debug/cocobeat-resample-preflight" "$probe_output/resample" > "$probe_output/resample-results.jsonl"
```

`limits.py` 的退出 0 表示测量完成，需查看 `results.json` 的结构状态与波形指标；seek runner 对正例失败返回非零，原生失败与适配成功保存在各自输出目录

2026-10-03 的 High 质量矩阵复用同一工具，质量门槛失败时返回非零；以下 OxideAV 长曲脚本采用每进程 2 GiB 虚拟地址空间限制，输出目录必须尚不存在，成功仅表示结构检查通过，编码失败返回非零并保留 `result.json`

```sh
cargo test --offline --locked --manifest-path "$probe_root/oximedia-resample/Cargo.toml" --target-dir "$probe_build/oximedia-resample"
cargo build --offline --locked --release --manifest-path "$probe_root/oximedia-resample/Cargo.toml" --target-dir "$probe_build/oximedia-resample" -j 1
"$probe_build/oximedia-resample/release/cocobeat-resample-preflight" quality "$probe_output/resample-high" High > "$probe_output/resample-high-results.jsonl"
python3 -B "$probe_root/oxideav/limits.py" "$probe_output/oxideav-600s" "$probe_build/oxideav/release" --frames 28800000 --quality 0.5 --encode-seconds 3600
```

以下追加近满幅 q10 与 libvorbis 参照，后者要求 FFmpeg 已启用 libvorbis；比较器流式计算同位置的逐声道峰值、RMS、全曲和去掉首尾各 2048 帧后的 SNR/RMSE，拒绝不等长、不完整帧及非有限样本

```sh
near_source="$probe_output/rusty-limits/near-full/input.f32le"
q10_case="$probe_output/rusty-limits/near-full-q10"
oracle_case="$probe_output/rusty-limits/libvorbis-near-full-oracle"
"$probe_build/rusty-vorbis/release/adapter" "$q10_case" 48000 10 near-full
cmp "$near_source" "$q10_case/input.f32le"
mkdir -p "$oracle_case"
ffmpeg -v error -nostdin -y -f f32le -ar 48000 -ac 2 -i "$near_source" -c:a libvorbis -q:a 5 "$oracle_case/encoded.ogg"
for case_dir in "$q10_case" "$oracle_case"; do
  "$probe_build/rusty-vorbis/release/readback" "$case_dir/encoded.ogg" "$case_dir/symphonia.f32le"
  ffmpeg -v error -nostdin -y -i "$case_dir/encoded.ogg" -f f32le -c:a pcm_f32le "$case_dir/ffmpeg.f32le"
done
oxide_case="$probe_output/oxideav-near-full/cases/n48000-q0.5-near-full"
cmp "$near_source" "$oxide_case/input.f32le"
for case_dir in "$probe_output/rusty-limits/near-full" "$q10_case" "$oracle_case" "$oxide_case"; do
  for decoder in symphonia ffmpeg; do
    python3 -B "$probe_root/rusty-vorbis/limits.py" compare "$near_source" "$case_dir/$decoder.f32le" > "$case_dir/compare-$decoder.json"
  done
done
```

## 首轮持久化验证

三个工具包在独立 `/tmp` 构建目录以 `--offline --locked --release -j 2` 构建通过；重新运行 OxiAudio 四种边界、OxideAV 1025 帧、rusty baseline 八例及适配器短样本五例和脉冲，共享 reader 与 FFmpeg 保持各自原 PASS/FAIL 帧数结果

三个 lock 与原文件逐字节相同；OxiAudio 四例原始输入及所有质量输出、OxideAV 1025 帧、rusty baseline/adapter 1025 帧与适配脉冲的输入、Ogg、两路回读 PCM 逐字节匹配原证据；没有因路径迁移重跑 206 秒长实验，历史 64 秒结果与源码身份由观察清单保存

分析器的 NaN/±Inf 检查已修复：保留 `nonfinite` 失败计数，把无效 RMSE/SNR 与预览值写为 JSON null，避免统计结果在落盘时丢失；一个标准库自检覆盖有限值不变和三种非有限值，历史全有限数值不变

原始全量材料仍在本地 `target/canonical-probe/`、`target/oxideav-probe/`、`target/rusty-vorbis-probe/`；PCM/Ogg、第三方源码、crate archive、编译缓存和大日志不进入 Git，仓库保留原创生成/分析代码、锁文件与小型固定观察清单

后续实测原始材料为 `target/rusty-vorbis-limits/`、`target/oxideav-near-full/`、`target/canonical-seek-probe/` 和 `target/resample-preflight/`；Rusty 原短样本、脉冲与 64 秒共 28 个产物逐字节回归通过，OxideAV 原 1025 帧输入及 Ogg 字节不变；新增研究依赖单列于 [研究台账](../licenses/CANONICAL_PROBE_DEPENDENCIES.csv)，不并入产品依赖

最终比较器未重新编码，复算四组编码结果的双路 PCM 共八路，SNR/RMSE 与历史记录最大差 1.17e-12；自检覆盖已知 6.0206 dB、RMSE、跨块统计及损坏输入拒绝
