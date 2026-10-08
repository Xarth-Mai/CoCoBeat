# 原生 tempo 实验观察

```sh
cargo run --locked -p cocobeat-lab -- inspect-native-tempo /path/to/package left /path/to/new-tempo.json
```

`inspect-native-tempo PACKAGE left|right NEW_REPORT` 只读完整 SongPackage，从同一次拥有并验证的最终 48 kHz stereo canonical PCM 快照选择左或右声道，不下混、不重编码。报告保留本次完整来源 CID、audio BLAKE3、N 与固定 profile `btt-48k-tempo-only-v1`，保存前重新验证来源；输出须为包外的新文件，父目录已存在，已有目标拒绝覆盖，JSON 上限 128 MiB，超限报错不截断

BTT 输入由真实 128 帧块组成，解码 callback 的分块不改变观察位置，EOF 只处理实际剩余尾长，不补零。每行 `consumed_frames` 是已经消费的 PCM 帧数，最终一行等于 N，记录条数为 `ceil(N / 128)`；这是消费位置，不是事件时刻，不能把它当 beat / onset 或 TempoRegion 边界。沿用现有十分钟 canonical 输入上限，最多 225000 行；选定声道样本须有限且位于 [-4, 4]，不裁切或归一化

每行保留原 `bpm`、整数 `period_frames` 和 histogram `native_certainty`，另列 `warmup_complete`、真实选定声道连续零样本数与 stale_status。零判断只针对准确等于零的实际样本，不代表听觉静默；`stale_after_full_zero_support` 是保留 tempo 已超过固定零支持窗口的诊断标记，不替换原 BPM / certainty，也不把未变化的估计当作可靠 TempoRegion

固定配置为 1024 FFT、128 hop、15 filter、1024 OSS / threshold / CBSS、零 callback latency 参数，无 callback / 可配置 setter。报告 confidence、beat_unit、meter 均为 null，质量为 `UNSCORED_NO_ADMISSION_THRESHOLD`、production_admission=false；BPM 不默认 quarter / eighth 或拍号，raw certainty 不等于校准置信度。此命令不修改 SongPackage analysis、Ready、Anchor、StagePlan 或 Replay

## 原生源码与许可

专用 `cocobeat-btt` 绑定采用 [Beat-and-Tempo-Tracking 固定提交 c039090f1af771092d95c3ffc402e557940f7384](https://github.com/michaelkrzyzaniak/Beat-and-Tempo-Tracking/tree/c039090f1af771092d95c3ffc402e557940f7384) 的六个 C 源、六个必要头文件与 MIT LICENSE，位于 `crates/cocobeat-btt/vendor/btt/`，十三份文件保持上游字节。自有 Rust 绑定 / 固定 C shim / 构建接线采用 MPL-2.0；完整 MIT 原文和逐文件来源见[许可目录](../licenses/btt/README.md)及[SOURCES.json](../licenses/btt/SOURCES.json)，随发行包保留许可证与版权

unsafe FFI 仅位于绑定私有模块，C 句柄归单一 Rust owner 管理，不公开 raw pointer / setter / callback，media 和其他 workspace crate 保留 unsafe forbid 边界。GNU Linux 构建使用 gnu99 / libm；Windows 因上游 C99 VLA 需要 Visual Studio 的 clang-cl，产生目标 MSVC ABI 静态库，发布程序不要求运行时编译器。Windows release workflow 已加入预装 C++ Clang 组件检查，固定 `8cdda9d` 的两架构原生优化构建与解包 Lab 软件运行已通过，范围见下文同提交四目标记录

## 软件状态

本批 Linux x86-64 的 BTT 2 项、media 67 项、Lab 80 项、xtask 6 项、Clippy / 格式 / 依赖边界和 metadata 检查通过，包含新增 2 绑定 / 3 media / 1 Lab 窄测；492 项构建输入前后及最终工作区一致，冻结 Lab SHA-256 为 `886556450c756abcd3c50a8a0a9a9ac53fc516a470a9b48c75cf458e2bf56257`

实际第二轮五个进程 exit0、正常退出且输入保持：从同一完整验证的 32 秒最终包严格导出 canonical PCM，再对左右声道分别运行正式 Lab 与旧 C driver。每声道 12000 个共同消费坐标、BPM / certainty 的 binary64 原位及其余字段精确值全部一致，差异为 0；最终 postflight 完整相等。首轮整体 FAIL 保留，唯一漂移是 QA 目录中运行期间新增审查 JSON，原五进程均 exit0、两声道原值一致，不能据此覆盖首轮整体结果

本样本 N=1536000，能整除 128，实际 EOF 尾余为 0；本次没有覆盖 native 部分尾等价性，编译参数等价仍未确认。单包原值一致是软件 ABI / 消费协议观察，不证明 tempo 正确、节拍单位、拍号或一般音乐质量。旧 40 条 source / canonical BTT [研究](../tools/native-tempo-check/results-2026-10-07.json)没有设质量准入阈值，13/20 编码前后 tempo 曲线发生变化；旧软件 / sanitizer 结果与本次新绑定验证分开

完整来源、两轮结果与实际编译 / 原始字段见[观察记录](../testdata/synthetic/native-tempo-observations-20261008.json)及[原始证据索引](../testdata/synthetic/native-tempo-observations-20261008-raw-index.json)。该本机批次的四目标发行运行当时 NOT_RUN，后续同提交四目标结果另列下文；发行 Action 每目标共 12 条 Lab smoke，其中 2 条为左右声道 tempo 检查；native 部分尾等价、长曲 CPU / RSS、实际音乐语义与设备 / 真人另验。旧其他 MIR / wholeSpect / 音乐质量 FAIL 保留，confidence / unit / meter 未知、UNSCORED / production_admission=false 和无可靠 TempoRegion 保持

## 同提交四目标发行软件验收

原 `07830f8` 四目标 FAIL 单独保留：Windows 两架构因未声明的 `random()` 构建失败，Linux 两架构完成构建后在首次实验导入的 Ctrl+C 注册处失败，tempo 和后续命令未运行，见[原 078 四目标失败观察](../testdata/synthetic/release-four-target-07830f8-observations-20261008.json)及[原始证据索引](../testdata/synthetic/release-four-target-07830f8-observations-20261008-raw-index.json)；更早 `0930d17` 三目标与 `d3b2648` 单 ARM 的历史矩阵保持各自源码范围

Windows 编译参数仅在 MSVC 构建将 `random` 映射到 `rand`，固定 BTT / shim 路径不调用该 statistical RNG helper，vendor 源字节与 Linux 参数保持；Lab SIGINT 接管见[取消说明](native-beat-candidate.md#ctrlc-取消原生候选导入)

固定 `8cdda9d` 通过 CI 后，Windows / Linux × x86-64 / ARM64 四个原生优化 Game / Lab 包及解包软件检查通过，48 条实际 Lab 命令为 44 条成功和 4 条预期缺失模型拒绝，八份左右声道 tempo 报告共 192000 行完成核验；包内源码、资源、许可台账与字体 QA 排除按该提交身份匹配，见[同提交四目标观察](../testdata/synthetic/release-four-target-8cdda9d-observations-20261008.json)及[原始证据索引](../testdata/synthetic/release-four-target-8cdda9d-observations-20261008-raw-index.json)

Linux ARM64 本次 `--jobs 1` 构建的 GNU time wall 为 13:03.42、最大 RSS 11453596 kbytes，属于单次 runner 环境观察，不作为预算或旧失败根因。此矩阵不验收 Game GUI、干净机器 / Windows VC 前置、实体输入 / 音频、真人音乐参考或实际 tag Release；旧 wholeSpect 19 PASS / 9 FAIL（整体 FAIL）与音乐质量 FAIL 保留，tempo 的 beat_unit / meter / confidence 仍为 None、质量 UNSCORED、production_admission=false
