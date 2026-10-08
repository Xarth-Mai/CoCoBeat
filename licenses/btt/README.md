# BTT 固定原生源码与许可

产品通过专用 `cocobeat-btt` 绑定使用 [Beat-and-Tempo-Tracking](https://github.com/michaelkrzyzaniak/Beat-and-Tempo-Tracking) 的固定原生实现，输入为最终 canonical 音频的显式单声道 48 kHz PCM，输出原始滚动 tempo 观察，不生成可靠 TempoRegion 或修改歌曲包

## 上游身份

固定提交为 [c039090f1af771092d95c3ffc402e557940f7384](https://github.com/michaelkrzyzaniak/Beat-and-Tempo-Tracking/commit/c039090f1af771092d95c3ffc402e557940f7384)，日期 2023-12-18，提交标题 Update Filter.c；2026-10-08 只读核对官方 master API 仍返回该提交，本次沿用已有原生研究验证的固定版本

六个 C 源文件、六个必要头文件与原 LICENSE 位于 `crates/cocobeat-btt/vendor/btt/`，十三个文件全部保持上游字节；[SOURCES.json](SOURCES.json) 逐项记录仓库根相对路径、上游路径、Git blob、字节数与 SHA-256，`modifications` 为空

六个实际编译单元为 BTT.c、DFT.c、STFT.c、Filter.c、Statistics.c、fastsin.c，依赖闭包只有对应头文件、自有 DSP / FFT、C 标准库和 GNU Linux 的 libm；没有采用 demos、Python binding、额外模型或外部 FFT 库

## 许可与分发

[LICENSE](LICENSE) 是 [上游固定提交的 MIT 原文](https://github.com/michaelkrzyzaniak/Beat-and-Tempo-Tracking/blob/c039090f1af771092d95c3ffc402e557940f7384/LICENSE)，完整保留 `Copyright (c) 2021 michaelkrzyzaniak`，SHA-256 为 `74400a6ea5b29562c7b128ff1ae0921e8f39af5c56f65e569c3bc8d69fefe829`，与 vendor/btt/LICENSE 逐字节相同

自有 Rust 所有权绑定、固定 C shim 与构建接线采用 MPL-2.0，crate 声明 MPL-2.0 AND MIT；MIT 授权与版权原文随二进制发行包的 licenses 目录保留，源码分发同时保留 vendor 源文件原有版权与本来源记录

`cc 1.5.1` 只为该绑定增加直接 build 依赖，复用 Cargo.lock / THIRD_PARTY.csv 已有 registry 包，许可为 MIT OR Apache-2.0；BTT 固定 native 源不是新增 Cargo registry 包，来源单列于本目录

## 构建与能力边界

安全接口固定 1024 FFT / 8 overlap / 15 filter / 1024 OSS / 1024 threshold / 1024 CBSS / 48000 Hz / 零 callback latency 参数，不注册 callback，不公开配置 setter 或 raw C handle；每次最多借用 128 个实际样本，EOF 不补零

Windows 使用 clang-cl 编译原有 C99 VLA 并产生目标 MSVC ABI 静态库，不修改数组或 setter；GNU Linux 沿用 gnu99 与 libm，工具链只在构建时需要；x64 / ARM64 的实际编译、链接及运行仍须分别验收，源码和许可收录不作为四目标 PASS

Windows 编译参数将 POSIX `random` 映射为标准 C `rand`，用于编译当前固定 BTT / shim 路径未调用的三个 statistical RNG helper；上游源码字节保持，Linux 不使用此映射，不宣称两种 RNG 的序列等价

histogram certainty 保持原值，confidence、beat unit 和 meter 为未知，报告固定 UNSCORED_NO_ADMISSION_THRESHOLD 与 production_admission=false；已有 Linux 原生研究的结果不替代当前绑定的新软件验证或真实音乐准入
