# Native BTT tempo check

固定 [BTT MIT 源码](https://github.com/michaelkrzyzaniak/Beat-and-Tempo-Tracking/tree/c039090f1af771092d95c3ffc402e557940f7384) `c039090f1af771092d95c3ffc402e557940f7384` 的 Linux 原生软件预准入工具，六个 C 文件及对应头文件保持原样，依赖闭包为自身 DSP / FFT 与 C 标准库 / libm；原许可证在 prepared 的 `btt/LICENSE`，自有工具沿用 MPL-2.0

Linux x86_64 的固定源码构建、边界控制和 native / sanitizer 各 40 条软件观察已完成，质量仍为 `UNSCORED_NO_ADMISSION_THRESHOLD`、`production_admission=false`；长期成本与其余原生目标未执行，纯 Python stdlib 只负责 QA 取证与统计，不是生产分析后端，未引入 Cargo / media / schema / FFI 或新 codec

## 固定算法与原值

实际调用 `btt_new(1024, 8, 15, 1024, 1024, 1024, 48000, 0, 0)` 与 `BTT_ONSET_AND_TEMPO_TRACKING`，不注册 callback，其余参数保留上游默认；FFT / overlap / filter / history 维度采用上游建议，48 kHz 为显式采样率配置，两项 0 仅用于未消费的 callback 延迟参数，不是默认 44.1 kHz callback 已校准的声明

每声道创建独立对象，F32LE 按小块借用后立即处理，输出原生 `btt_get_tempo_bpm`、整数 `btt_get_beat_period_audio_samples` 和原始 `btt_get_tempo_certainty`；certainty 是 histogram 值，`confidence=null`，不做归一化、平滑、半倍速修正或拟合平移

默认每 128 帧保留一次原始滚动估计；QA 的 1 / 128 / 1024 分块大小只改变输入调用粒度，分别在可观察的共同消费位置比较，最后不足整块的输入也保留末次读取；`consumed_frames` 是读到的 PCM 位置，可能等于 N，不是 `[0,N)` 的事件坐标

原生 tempo 在 1024 个 OSS hop 后才开始，`warmup_complete` 的边界为 131,072 PCM 帧 / 2.730667 s；短输入返回完整的原始 0 估计与未完成热身标记，不能解释为静默、没有节拍或已完成分析

driver 记录实际输入的连续精确零样本数；原生全零 OSS 会提前返回并可能保留旧 BPM，只有精确零持续超过保守支持长度 `1024 + 128 + (15 + 1 + 1024) * 128 = 134272` 帧且 BPM 非零时，才标记 `stale_after_full_zero_support`，其中计入 STFT、前谱、FIR 和 OSS 历史；这是基于固定实现的 stale 诊断，不修改估计或建立置信门控，较短尾部标记 `insufficient_zero_history`

精确 PCM 零不等于听觉静默，编码后微小非零尾巴不擅自按阈值归零；现有样本的尾部若不足上述长度，只报告实际零长度和旧 BPM，不声称已验证完整静默后的清空行为，也不追加静默或重写旧音乐来制造通过

## 输入与输出边界

- 输入为只读 stereo F32LE regular file，固定 48 kHz、`1 <= N <= 28,800,000`、长度恰好 `N * 8` bytes；所有声道样本均须 finite 且在 `[-4,4]` 内，逐块读取，不下混、不缓存全曲 PCM
- driver 仅接受固定 QA block 大小，不开放 BTT 参数；检查原生 BPM / period / certainty 的有限性、整数周期范围和热身行为，不将越界值静默裁剪
- 输出使用 `O_EXCL` 创建，已有路径与输入文件不能覆盖；流式输出失败时保留部分证据，必须同时有真实 exit 0、最后 `complete` 记录和完整结构校验才能当作软件完成
- 当前 driver 使用 Linux POSIX 文件 API；上游有 VLA 和 `M_PI`，Windows MSVC 直接编译未验证，Linux ARM64 与 Windows x86_64 / ARM64 保持 NOT RUN，不能用本机 Linux x86_64 研究代替四目标证明
- BTT 内存随固定窗口配置分配，不随曲长增长；这只是源码属性，OOM / 600 s 成本和资源中断行为仍需实测，不能把当前短矩阵 sanitizer 结果扩展为所有输入安全证明

## 原输入与标量参考

复用 Beat This! 研究已有 `source-matrix` 和严格 `canonical-matrix` 各 10 份 PCM、左右声道，共 40 条记录；原 SHA、beat 标签、单位、所有固定 / 非整数 / accelerando / 3/4 / 6/8 / 弱起 / swing / 静默 / 反相 / 单边声道保持，canonical 来源绑定媒体 driver `199953f3…` 和旧 encode / strict readback receipts，本工具不重新编码或生成音乐

`prepare.py` 只核小文件 SHA、Git blob、来源关系和 PCM 的存在 / 长度，复制小型快照，不在性能窗口读取大 PCM；实际运行前后必须匹配旧 PCM SHA，原输入缺失或漂移直接拒绝，不用重生成输入替代当前已存在的原文件

标量参考只在原标签的相邻区间 `[round(beat[i]*48000), round(beat[i+1]*48000))` 定义为 `60*48000/(end-start)`，它是该间隔的平均 BPM；以实际读取位置对照，不移动预测或标签，不延伸首个 beat 之前和最后 beat 之后的参考，热身期单独保留而不评分

稳定类别来自旧生成配方，固定 / 非整数 / 拍号变体 / 弱起 / swing / 声道变体均保持原主 beat 单位；accelerando 按各实际相邻区间报告，不把分段均值冒充连续瞬时真值；6/8 的单位仍是 `dotted_quarter`，其余为 `quarter`，不会自动转成四分音符或八分音符，`silent_channels` 不派生 BPM 参考

旧 metadata 的 `generator_sha256` 是当时完整脚本的身份，当前只复制用于解释配方的 `probe.py` 并独立记录其 SHA，不冒称两份完整源码相同；本批不调用生成器，实际输入身份由旧 PCM SHA 决定

统计给出有效覆盖率、未折算的相对误差分布 / BPM 比率、首次非零估计、静默声道非零数量、尾零长度及 stale 诊断；分位数采用 nearest-rank，空分布为 null；当前没有既定 tempo 准入门槛，固定 `UNSCORED_NO_ADMISSION_THRESHOLD` 和 `production_admission=false`，即使全部进程成功也不标质量 PASS，前期方案中提出的数值门槛未启用

tempo 估计不代表精确 onset、beat 坐标、meter、可靠 TempoRegion、section / repetition 或可玩 Anchor；原 onset Matcher 不修改，未知能力不能以空集合伪装支持

## 准备与后续 CPU 窗

轻量准备仅执行下列命令，输出目录必须不存在；冻结 index 包含原 BTT、工具、旧 metadata、来源 receipts 和明确统计规则

```bash
python -B tools/native-tempo-check/prepare.py target/native-tempo-20261007/prepared-v1
```

下一 CPU 窗再执行 pilot，总上限 60 s、所有子进程串行且各不超过剩余预算 / 30 s；它包括 native / ASan+UBSan 两次构建、各 16 项输入契约控制、原 `fixed_120` 整个 32 s 左声道的 native / sanitized 及 1 / 128 / 1024 分块 / fresh-object 重复比较，不生成新音乐

```bash
prepared="$PWD/target/native-tempo-20261007/prepared-v1"
python -B "$prepared/frozen/check.py" pilot "$prepared" "$PWD/target/native-tempo-20261007/pilot-v1"
```

同一 native 二进制的共同位置结果要求完全相同，sanitized 不同优化配置只对 BPM / certainty 使用预先声明的 `atol=1e-6, rtol=1e-6`，整数周期和其余字段必须完全相同；这是软件数值检查，不是质量门槛

原生命令、stdout / stderr、真实退出码、耗时和输入 / 二进制 / 输出 SHA 均独立留存；build 日志含实际编译参数，二进制旁绑定冻结源码 index；超时只终止并回收当前 owned process group，保留不完整证据后停止，不自动重试或忽略 sanitizer 错误；`ASAN_OPTIONS=detect_leaks=1:halt_on_error=1`、`UBSAN_OPTIONS=halt_on_error=1` 固定，环境初始化失败单独保留，不换 flag 粉饰

pilot 的单声道实际耗时只用于粗估相同长度 40 条成本，其他节奏、hash 与统计的开销仍需计入；完整矩阵由主线程按 pilot 决定再运行，脚本总上限 300 s，不由 pilot 自动启动

```bash
python -B "$prepared/frozen/check.py" matrix "$prepared" "$PWD/target/native-tempo-20261007/pilot-v1/build-native/native-tempo-check" "$PWD/target/native-tempo-20261007/matrix-native-v1"
```

Linux `RUSAGE_CHILDREN` 给出的 RSS 是执行器启动以来所有已回收子进程的高水位，字段明确标注该范围，不作为每个 driver 独立峰值；本轮未改 CI、生产分析或共享文档

## 2026-10-07 实际结果

`prepared-v1` 冻结 25 项文件，index SHA `0acd03d0…`，其中 14 项上游小文件的 Git blob / SHA 保持官方 commit 原值；实际 Linux native / sanitized 构建均成功，二进制 SHA 分别为 `1175bf8d…` / `89922d57…`，全部源码、原输入和二进制前后身份一致，快照中的准备期 README 保持历史原样

60 s pilot 实际 11.213 s，完成两次构建、native / sanitized 各 16 项契约控制与旧 fixed_120 左声道的五次运行；分块共同位置与 fresh-object 重复一致，native / sanitized 完整输出逐字节相同，没有 sanitizer 诊断，所有预期拒绝保留真实 exit 2

随后 native 40 条实际 56.933 s、sanitizer 40 条实际 110.965 s，全部 exit 0，无超时；40 对原始 JSONL 逐字节相同，两种构建的 80 份矩阵 stderr 均为空，原参数 `detect_leaks=1` 保持，所有 owned child 已回收；同期有其他 Cargo 工作，时间仅为这批非独占软件研究成本

34 条非静默声道在有参考区间且热身后均有非零估计，覆盖率 100%；6 条静默声道没有非零 BPM，原始整数 lag、certainty 和尾零信息全部保留；稳定 120 BPM、3/4、弱起和 swing 的相对误差 P95 约 0.267%，非整数 123.45 BPM 约 0.142%，该 6/8 样本保持原附点四分音符单位，90 BPM 的 P50 / P95 误差为 0

accelerando 的相对误差 P50 为 4.571%、P95 为 5.496%，原始估计落后于各参考间隔，未拟合延迟或改标签；这些合成统计没有证明真实音乐、6/8 / swing 的一般消歧、可靠拍号或完整 TempoRegion，也没有据此新建质量 PASS 门槛

源与 canonical 的 20 对里，7 对 BPM 全序列相同，13 对有整数 lag / BPM 变化；即使部分汇总分位数相同，也不表示编码前后逐点一致，各对变化数和原始 SHA 均保留；非静默样本尾零不足 134272 帧，因此只观察到旧 BPM 与 `insufficient_zero_history`，有声转完整静默后的 stale 行为仍未实际验明

小型持久摘要见 [results-2026-10-07.json](results-2026-10-07.json)，完整源码、命令、stdout / stderr、原始曲线和逐项 SHA 位于 `target/native-tempo-20261007/`；软件研究完成，生产依赖 / FFI 接入、置信度校准、精确 onset、beat 坐标、meter、section / repetition、600 s 与其余目标仍未准入
