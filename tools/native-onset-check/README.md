# Native AudioFlux onset check

纯 C 研究 driver，固定 AudioFlux MIT 源码 `0c3f55b409b07381bfe770711e3642e19f333bee`，仅编译内置 FFT 路径，不引入 Python、FFTW、OpenMP、模型或新 codec 到产品；当前为软件准入前置，尚未接入生产 media

`driver.c` 使用 48 kHz、1024 帧 Hann、128 帧 hop、无 padding、RCD、`filterOrder=1` 和上游默认 peak picker；整数坐标为原生 STFT index × hop，不补首尾或平移结果；`novelty` 为完整归一化分数序列，`confidence=null`，不将分数当概率

上游约 30 ms 的 `preMax` 与 `wait`、`delta=0.07` 保持不变，预期存在 8 ms 近邻限制；31 项离散评分和 3 项连续观测全部保留，单样本输入显式 `FAIL_UNSUPPORTED`，不能用空数组把它变成成功分析

## 边界与预算

- 输入只读 F32LE regular file，显式声明 1 / 2 channels、channel、N，文件长度必须恰好等于 `N×channels×4`，每个声道全部样本须 finite 且位于 `[-4,4]`，不下混
- N 上限 64 秒，短于 1024 帧在完成 PCM 校验后明确拒绝，无隐式 padding；10 分钟输入当前明确不支持，不以 64 秒测试代替长期成本验收
- 保守工作分配上限 512 MiB，使用固定维度先检查 C int 与分配乘法；上游内部未完整处理 OOM，这仍是生产接入前需处理的问题，源检查不能保证原生库绝不会异常终止
- 输出通过 `O_EXCL` 创建，拒绝已存在文件，不覆盖输入；在计算及完整结果校验后才创建输出，写失败的部分文件保留为失败证据
- 当前文件接口为 Linux C99 / POSIX，Windows 与 ARM 原生执行尚未验证；四目标接入另审，不将系统 C 库能力误称为跨平台证明
- 首个 CPU 窗上限 300 秒、所有执行串行，每个 driver / matcher 子进程最多 5 秒；不生成新音乐、不编码、不下载、不启动 GPU，超时保留失败并停止扩大矩阵

## 固定输入与原 Matcher

`prepare.py` 是 stdlib QA 源码取证工具，只复制已核验的 23 项 AudioFlux 文件和本工具，不做编译或分析；从旧 Rust 源码逐字提取原生成函数、原 `metrics` 和原测试，记录精确行号与片段 SHA；四份旧源码 SHA 与 2026-10-03 持久观察记录一致

原 34 项依赖的 29 份 PCM 在首次准备时缺失，总量 13,104,020 bytes，现已在新的 owned 目录完整恢复并匹配全部旧 SHA；生成器使用旧种子、参数、浮点运算和配方，所有文件必须匹配冻结声明里的旧 SHA，才允许候选矩阵启动；浮点差异或缺失不能冒充旧字节，原标签不重新推导

`fixture-matcher.rs` 不依赖 OxiMedia 或 Cargo，使用已构建的精确 `serde_json` rlib 与其 dependency 目录，通过 rustc 编译；它的 `score` 入口调用逐字复用的原 Matcher，保留 ±480 帧 / ±10 ms、median≤96 帧 / 2 ms、全匹配零额外峰与连续观察 null 指标

现有 Beat This! strict canonical 10 项回读文件是另一组样本，不能拿 beat 标签当 onset 真值；本批旧 34 项的同源 canonical 回读仍 NOT RUN，后续按独立来源与作用记录，不能冒充编码前后同矩阵

## 准备与待执行命令

以下准备只写源码快照，不编译、不生成 PCM；输出目录必须不存在

```bash
python -B tools/native-onset-check/prepare.py target/mir-permissive-20261007/candidate-driver/prepared-v1
```

取得 CPU 窗后，使用准备目录中的冻结脚本，所有 build / evidence 路径必须是新的 owned 目录；命令 stdout、stderr 与退出码由调用方保留，下面是首次执行所用路径，再次复现须换成新的目录

```bash
prepared="$PWD/target/mir-permissive-20261007/candidate-driver/prepared-v1"
work="$PWD/target/mir-permissive-20261007/candidate-driver"
timeout 120 bash "$prepared/frozen/tools/native-onset-check/build.sh" "$prepared" "$work/build-native-v1" native
timeout 120 bash "$prepared/frozen/tools/native-onset-check/build.sh" "$prepared" "$work/build-sanitized-v1" sanitized
```

生成器与测试使用主线程冻结 Cargo artifact JSON 中实际 `serde_json` rlib，不用 glob 猜一个旧文件；将其绝对路径传为 `serde_json_rlib`，记录该文件、rustc 版本、harness 和命令 SHA，以下是待填入精确 artifact 的命令模板

```bash
rustc --edition=2024 --extern "serde_json=$serde_json_rlib" -L "dependency=$(dirname "$serde_json_rlib")" "$prepared/fixture-matcher.rs" -o "$work/fixture-matcher-v1"
rustc --edition=2024 --test --extern "serde_json=$serde_json_rlib" -L "dependency=$(dirname "$serde_json_rlib")" "$prepared/fixture-matcher.rs" -o "$work/fixture-matcher-tests-v1"
"$work/fixture-matcher-tests-v1"
"$work/fixture-matcher-v1" regenerate "$work/fixtures-v1"
python -B "$prepared/frozen/tools/native-onset-check/check.py" controls "$work/build-native-v1/native-onset-check" "$work/controls-native-v1"
ASAN_OPTIONS=detect_leaks=1:halt_on_error=1 UBSAN_OPTIONS=halt_on_error=1 python -B "$prepared/frozen/tools/native-onset-check/check.py" controls "$work/build-sanitized-v1/native-onset-check" "$work/controls-sanitized-v1"
python -B "$prepared/frozen/tools/native-onset-check/check.py" matrix "$work/build-native-v1/native-onset-check" "$work/fixture-matcher-v1" "$prepared" "$work/fixtures-v1" "$work/matrix-native-v1"
ASAN_OPTIONS=detect_leaks=1:halt_on_error=1 UBSAN_OPTIONS=halt_on_error=1 python -B "$prepared/frozen/tools/native-onset-check/check.py" matrix "$work/build-sanitized-v1/native-onset-check" "$work/fixture-matcher-v1" "$prepared" "$work/fixtures-v1" "$work/matrix-sanitized-v1"
```

软件 controls 只构造输入契约控制：1024 帧静默、±4、next-up(4)、非有限、未选声道非法值、长度不符、短输入、超预算和已有输出保护，不生成音乐真值；质量矩阵只使用恢复后 hash 完全匹配的历史输入，质量 FAIL 退出 1，拒绝或异常与质量结果分别记录

ASan 保留默认虚拟地址空间供 shadow mapping，不套小 `RLIMIT_AS`；实体内存、超时、native stderr 和真实退出码单独观察，不能把 sanitizer 初始化失败当库通过

自有工具沿用 MPL-2.0，复用原创 PCM / 构造标签沿用旧 CC0-1.0；AudioFlux 许可证原文在准备目录 `audioflux/LICENSE.md`，第三方源保持未修改，源码和全部执行输出分别记录身份

## 2026-10-07 实际结果

Linux x86_64 的 native / sanitizer 两种 C 构建、原 Matcher 单测与 12 项 native 输入边界控制通过，29 份历史 PCM 全部按旧 SHA 恢复；固定 RCD / 默认 peak picker / 原始索引坐标下，31 项离散结果为 2 PASS、28 FAIL、1 项 `FAIL_UNSUPPORTED`，另 3 项连续观测不评分，生产准入继续 FAIL

默认 wait 在该 hop 下要求候选至少相隔 1536 PCM 帧 / 32 ms，两个 8 ms 近邻用例各只输出一个候选；两个 32 ms 用例也失败，不能只用 wait 解释后者。常规 120 BPM 脉冲的预测按序比源事件提前 832 / 896 帧，均超出 Matcher；首尾用例只输出 `[23168]`，truth 为 `[0,24000,47999]`，partial tail 无候选，1 帧输入明确不支持。按序偏差仅为诊断，不代替正式匹配指标，未平移输出、修改参数或补置信度

首次 sandbox 执行的 sanitizer controls / matrix 在首例退出时遭遇 LeakSanitizer ptrace 环境 fatal，原 stderr 与退出 1 保留；随后在宿主环境使用完全相同的 sanitizer 二进制与 `detect_leaks=1:halt_on_error=1`、`UBSAN_OPTIONS=halt_on_error=1`，12 项 controls 和完整 34 项矩阵均完成，无 ASan / UBSan / LSan 诊断。矩阵退出 1 对应已完整记录的质量 FAIL，全部 34 项指标与 native 相同，33 份完整预测 / novelty 文件逐字节相同，1 项短输入保持同样拒绝

宿主复核实际约 3.05 秒，执行器最大子进程 RSS 为 52,724 KiB，仅代表这组短 QA 负载；native 34 项执行器约 1.17 秒，包含进程、文件与记录开销，不作为产品算法性能排名。源码、命令、二进制、恢复文件、所有原失败和最终结果保存在 `target/mir-permissive-20261007/candidate-driver/observations-20261007.json`，初始 38 文件快照保持原样，结果更新只修改本文

旧 34 项同源 canonical 回读、独立 FFT / RCD 数值 oracle、Windows / ARM、64 / 600 秒成本、真实音乐与人工标签仍 NOT RUN；本次 sanitizer 通过只覆盖实际输入和路径，不构成完整 MIR 或生产准入
