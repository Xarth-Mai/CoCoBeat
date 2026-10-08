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

unsafe FFI 仅位于绑定私有模块，C 句柄归单一 Rust owner 管理，不公开 raw pointer / setter / callback，media 和其他 workspace crate 保留 unsafe forbid 边界。GNU Linux 构建使用 gnu99 / libm；Windows 因上游 C99 VLA 需要 Visual Studio 的 clang-cl，产生目标 MSVC ABI 静态库，发布程序不要求运行时编译器。Windows release workflow 已加入预装 C++ Clang 组件检查，x64 / ARM64 的实际编译、链接与运行尚待本批独立结果，配置支持不作为四目标 PASS

## 软件状态

本批 Linux x86-64 的 BTT 2 项、media 67 项、Lab 80 项、xtask 6 项、Clippy / 格式 / 依赖边界和 metadata 检查通过，包含新增 2 绑定 / 3 media / 1 Lab 窄测；492 项构建输入前后及最终工作区一致，冻结 Lab SHA-256 为 `886556450c756abcd3c50a8a0a9a9ac53fc516a470a9b48c75cf458e2bf56257`

实际第二轮五个进程 exit0、正常退出且输入保持：从同一完整验证的 32 秒最终包严格导出 canonical PCM，再对左右声道分别运行正式 Lab 与旧 C driver。每声道 12000 个共同消费坐标、BPM / certainty 的 binary64 原位及其余字段精确值全部一致，差异为 0；最终 postflight 完整相等。首轮整体 FAIL 保留，唯一漂移是 QA 目录中运行期间新增审查 JSON，原五进程均 exit0、两声道原值一致，不能据此覆盖首轮整体结果

本样本 N=1536000，能整除 128，实际 EOF 尾余为 0；本次没有覆盖 native 部分尾等价性，编译参数等价仍未确认。单包原值一致是软件 ABI / 消费协议观察，不证明 tempo 正确、节拍单位、拍号或一般音乐质量。旧 40 条 source / canonical BTT [研究](../tools/native-tempo-check/results-2026-10-07.json)没有设质量准入阈值，13/20 编码前后 tempo 曲线发生变化；旧软件 / sanitizer 结果与本次新绑定验证分开

完整来源、两轮结果与实际编译 / 原始字段见[观察记录](../testdata/synthetic/native-tempo-observations-20261008.json)及[原始证据索引](../testdata/synthetic/native-tempo-observations-20261008-raw-index.json)。发行 Action 已接每目标共 12 条未来 Lab smoke，其中 2 条为左右声道 tempo 检查，本批完整四目标 GHA / 发行软件运行仍 NOT_RUN；实际本机 Linux x86-64 编译 / 链接与单包对照已通过，另三目标 native 运行另验；native 部分尾等价、长曲 CPU / RSS、实际音乐语义与设备 / 真人另验。旧其他 MIR / wholeSpect / 音乐质量 FAIL 保留，confidence / unit / meter 未知、UNSCORED / production_admission=false 和无可靠 TempoRegion 保持

后续固定 `07830f8` 发行检查中，[Windows x64](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37728486403)和[Windows ARM64](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37728489606)实际 Build FAIL，clang-cl C11 拒绝 Statistics.c 未声明的 POSIX `random()`；[Linux x64](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37728492508)完成构建与打包准备，但后台 Lab 首次导入在 Ctrl+C 注册处失败，tempo 检查尚未运行。这些原失败保留；Windows 编译参数现将 `random` 映射到 `rand`，仅涉及固定 BTT / shim 路径未调用的 statistical RNG helper，vendor 源字节及 Linux 参数保持；Lab SIGINT 接管修复见[取消说明](native-beat-candidate.md#ctrlc-取消原生候选导入)。修复后的同 ref 四目标构建与十二项 smoke 另验，不从本机测试推断平台 PASS
