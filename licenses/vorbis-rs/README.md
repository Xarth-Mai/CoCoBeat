# 原生 Vorbis 编码库许可来源

本目录收录 `vorbis_rs 0.5.6`、`aotuv_lancer_vorbis_sys 0.1.6` 与 `ogg_next_sys 0.1.5` 的完整适用许可原文，供产品及发行包保留；许可文件保持原始字节，LPC notice 按下述明确源码范围提取

## 许可文件

| 本地文件 | 官方来源与提取范围 | SHA-256 |
| --- | --- | --- |
| [BSD-3-Clause.txt](BSD-3-Clause.txt) | [上游固定提交 LICENSE](https://raw.githubusercontent.com/ComunidadAylas/vorbis-rs/6934784e7188e98dd053370c79604c72b201e97a/LICENSE)，已与 [v0.5.6/LICENSE](https://raw.githubusercontent.com/ComunidadAylas/vorbis-rs/v0.5.6/LICENSE) 逐字节核对一致 | `86c9ae504639a2a0e68f46f244a95daa9011b955aa0d02cf70f09c06d10e26e4` |
| [Vorbis-COPYING.txt](Vorbis-COPYING.txt) | `aotuv_lancer_vorbis_sys-0.1.6.crate` 内 `vorbis_vendor/COPYING`，完整复制 | `0c72574d90753e1eda1f21a83c1a04990e67b26bb56e01acf3ce241140518156` |
| [libogg-COPYING.txt](libogg-COPYING.txt) | `ogg_next_sys-0.1.5.crate` 内 `ogg_vendor/COPYING`，完整复制；与 aotuv sys 归档内同路径文件逐字节相同 | `d2ab5758336489da61c12cc5bb757da5339c4ae9001f9bb0562b4370249af814` |
| [lpc.notice](lpc.notice) | `aotuv_lancer_vorbis_sys-0.1.6.crate` 内 `vorbis_vendor/lib/lpc.c` 第 1–43 行原始字节，包含 Xiph 文件版权及 Jutta Degener / Carsten Bormann 完整 preserved notice | `dfa1023c2fdde7a51e5d68df06698c0e52122ca63c3d721c15e2d56775d2d20c` |

Rust 绑定及 sys 包采用 BSD-3-Clause，上游版权人为 Alejandro González；Vorbis COPYING 同时保留 Aoyumi 与 Xiph.org 的版权，libogg COPYING 保留 Xiph.org 的版权；`lpc.c` 由当前 sys 构建脚本实际编译，其额外 notice 明确要求原样保留，不能只以 COPYING 替代

三个实际 crate 归档均未附顶层 Rust binding LICENSE，故该原文从固定官方提交补齐；本目录并非用 README 声明代替完整许可

## 官方归档身份

2026-10-07 对 Cargo 已取得的 crates.io 归档核对 SHA-256，内容来源为以下固定版本

| 归档 | 官方下载 | SHA-256 |
| --- | --- | --- |
| `vorbis_rs-0.5.6.crate` | [crates.io 0.5.6](https://crates.io/api/v1/crates/vorbis_rs/0.5.6/download) | `49c5da94d280f7a27e8c937e9b73df2da3e23a2583f48471fd8fb4c72f9c1933` |
| `aotuv_lancer_vorbis_sys-0.1.6.crate` | [crates.io 0.1.6](https://crates.io/api/v1/crates/aotuv_lancer_vorbis_sys/0.1.6/download) | `5bc4fd1a61860d2f1198b60bedd30910eaffa978f1ee6214dfb24ac70d589225` |
| `ogg_next_sys-0.1.5.crate` | [crates.io 0.1.5](https://crates.io/api/v1/crates/ogg_next_sys/0.1.5/download) | `ed2d7a48e247c2bb07e633aefb65a38648ea58c7eedd4e4408a5861721ab049b` |

三个归档的 `.cargo_vcs_info.json` 均指向 `6934784e7188e98dd053370c79604c72b201e97a`；aotuv sys 另标记 `dirty: true`，因此 C vendor 的生产来源身份采用上述实际归档及哈希，不声称所有文件等同于 Git 提交树

Vorbis vendor 的 `symbian/config.h` 含 CSIRO notice，当前 Windows / Linux x86-64 / ARM64 编译路径不使用它；若以后分发完整 vendor 源码包，应继续保留归档内该文件及原文 notice

## 发行收录

二进制发行包应保留上述四份许可原文及本来源记录，源码分发还应保留原源码中的版权和 notice；共享第三方台账与四平台打包由主线程接入，本目录存在本身不代表已完成发行包验证

这些许可与来源核对不表示编码器的软件准入、跨平台执行或真人听感已通过
