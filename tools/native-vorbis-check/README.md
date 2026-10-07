# 原生 Vorbis 编码软件验证

此工具直接调用 `cocobeat-media::encode_canonical_audio`、实际 High 重采样及严格最终读回；第二个完整 reader 为 `vorbis_rs::VorbisDecoder` / libvorbisfile，FFmpeg native Vorbis decoder 仅作第三路开发期互操作检查，不进入产品路径

主线程在注册生产 adapter、取得依赖并完成 Cargo 构建后，按本轮 `--message-format=json` 的真实 artifact 选择准确 media / vorbis_rs rlib，以 `rustc --edition 2024 tools/native-vorbis-check/driver.rs -L dependency=<deps-dir> --extern cocobeat_media=<exact-rlib> --extern vorbis_rs=<exact-rlib> -o <new-driver>` 编译薄入口，保留命令、构建输入与二进制 SHA-256；本目录不建立另一套 codec crate 或复制生产算法

```sh
python3 -B tools/native-vorbis-check/check.py \
  target/native-vorbis-check/driver \
  target/debug/cocobeat-lab \
  target/native-vorbis-check/matrix
```

需要 Linux、Rust 编译器、Python/NumPy、GNU timeout 与开发期 FFmpeg；输出根目录必须尚不存在，不播放音频。每个编码及 reader 命令限 60 秒 / 2 GiB 地址空间，最大 RSS、墙钟及原始标准输出/错误逐例保留，历史 FAIL 不改写

## 输入与判断

旧 `fullband-observations-20261003.json` 的 19 例作为来源身份清单；9 个基础控制从已跟踪旧 adapter 提取原输入生成片段，以相同 Rust 浮点表达式重建，不调用失败的旧编码器；64 秒歌曲使用 lab 的唯一整数生成器并核对已知 WAV 及重采样 PCM SHA-256；首尾 impulses 沿用旧全频带工具的实际数组

所有标为历史复用的输入必须先匹配旧 `source_sha256`，漂移立即失败。历史 quiet / swapped / correlated / opposed 及部分核构造来源若不存在，单独记录 NOT RUN；新同类配方使用 `new-` 名称，不冒充旧字节或覆盖旧结论

新控制覆盖 quiet、交换声道、同相、反相、±4 正负边界、next-up 超域、非有限值；十分钟复杂输入由原始 64 秒 PCM 逐字节循环并在 600 秒结束，保留原配方的静音段，作为明确的新控制

重采样矩阵只实测 `1, 7, 8000, 11025, 16000, 22050, 32000, 44100, 47999, 48000, 48001, 64000, 88200, 96000, 176400, 191999, 192000 Hz` 的规范化反相阶跃，记录实际 High 输出峰值、完整帧数与编码结果；编码器每个实际 block 的 finite / ±4 检查覆盖其数值域，不把代表性矩阵或旧核证书说成所有 192000 个整数采样率的普遍峰值证明

软件检查包括编码成功、EOS=N、48 kHz / stereo、完整输入与两完整 decoder 的实际 PCM 字节数、有限性及同位置最大差；双 decoder 预设容差为 `1e-6 * max(1, decoded_peak)`，保留绝对差与实际门槛，允许较高幅度下的 float 舍入，不平移、拟合增益、裁幅或补零

既有 near-full 与新的低频 quiet / swapped / correlated / opposed 正弦，编码前固定同位置逐声道 SNR 至少 35 dB 的软件控制门槛；其他控制完整保留波形、每秒、首尾、峰值邻域及频段指标，不以有损差异或任意 SNR 代替真人听感。度量复用已有 fullband `measure`，其局部退步和有限 overshoot 仍保留

默认 FFmpeg 的 exit、实际完整帧数与错误另列 PASS_COMPLETE / FAIL_COMPLETE，不覆盖任何短文件自动 trim 失败；两完整独立 reader 的软件结构结果与 FFmpeg 互操作结果分别可见，不对 FFmpeg 输出截断、填充或伪造 N 帧

薄 driver 的 QA PCM 半成品保留用于诊断；生产 encoder 本身仍按合同清理本次失败 Ogg，已有输出及源文件保留。矩阵通过只能表示所列软件控制通过，不能先行声明候选已获得完整生产准入、跨平台执行或真人听感

## 许可交接

实际 crate 归档与固定上游许可已核对，完整原文及 SHA-256 收录在 [licenses/vorbis-rs](../../licenses/vorbis-rs/README.md)，包括 Rust binding BSD、Vorbis / libogg COPYING 和编译路径实际包含的 LPC 额外 notice；复制文件与归档内字节核对通过，绑定许可与固定提交及 v0.5.6 tag 下载原文一致

主线程负责产品共享台账和四平台发行包收录验证；许可核对不表示编码准入或发行包检查已通过

本目录 Python / Rust 源码采用仓库 MPL-2.0，新增合成 PCM 配方及原开发音乐按既有 CC0-1.0 约定；工具本身不引入新的音乐来源

## 原生 sanitizer 窄测

2026-10-07 本机 Clang 23 的 `address,undefined,float-cast-overflow` 工具烟测及修补后 11 项 codec 控制通过，覆盖 1 / 1024 / 1025 帧、首尾脉冲、近满幅、反相、±4 边界与超域拒绝、十分钟原创音乐；有效输出均完整双读回，原始未修及中间修补的四轮错误记录保留，见 `target/vorbis-admission-20261007/san-final/summary.json`

使用隔离 `CARGO_TARGET_DIR`，沿用锁定依赖和真实 media adapter，让 sys 构建脚本使用 Clang instrumentation；完整矩阵的普通构建及结果另行保留

```sh
env CARGO_TARGET_DIR=target/native-vorbis-sanitized \
  ASAN_OPTIONS=detect_leaks=0 \
  CC=clang \
  CFLAGS='-fsanitize=address,undefined,float-cast-overflow -fno-sanitize-recover=all -fno-omit-frame-pointer -g' \
  RUSTFLAGS='-C linker=clang -C link-arg=-fsanitize=address,undefined,float-cast-overflow' \
  cargo build --locked --offline --release -p cocobeat-media --message-format=json
```

从该次 JSON 取得准确 media / vorbis_rs rlib，以相同 linker / link-arg 编译已有 `driver.rs`，不复制生产编码流程；窄测优先复用普通矩阵生成的 1 / 1024 / 1025 帧、首尾 impulses、±4 常值 / 正弦、反相、近满幅和十分钟原创音乐输入，以新路径保存输出、命令、二进制哈希、实际 stderr 和退出码

ASan shadow memory 需要较大的虚拟地址空间，因此 sanitizer 命令单独执行，保留 60 秒 timeout 与实测 RSS，不直接套用 `check.py` 普通矩阵的 2 GiB `RLIMIT_AS`；真实编码、完整 decoder 回读及无 sanitizer 诊断均应逐例检查，原错误结果不覆盖

这些 flags 对 C vendor instrumentation，Rust 本体仍是普通编译；检测器可用、所列控制通过或未观察到错误，都不能写成 C 全输入域的形式安全证明

LeakSanitizer 因当前 sandbox 的 ptrace 限制未执行，`address,undefined,float-cast-overflow` 保持启用；Rust 本体未 instrument，测试通过只覆盖实测控制，四目标原生运行和真人听感继续分别取证
