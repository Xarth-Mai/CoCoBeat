# libogg sys 来源与官方修补

从 crates.io `ogg_next_sys 0.1.5` 官方归档完整提取，原始 21 个文件及 SHA-256 见 [UPSTREAM.json](UPSTREAM.json)，归档 SHA-256 为 `ed2d7a48e247c2bb07e633aefb65a38648ea58c7eedd4e4408a5861721ab049b`；新增顶层 LICENSE 从官方绑定项目固定版本补齐，来源见 [许可台账](../../licenses/vorbis-rs/README.md)

完整回移 [Xiph 官方修复 7cf42ea](https://github.com/xiph/ogg/commit/7cf42ea17aef7bc1b7b21af70724840a96c2e7d0)，原始补丁见 [cocobeat-backport.patch](cocobeat-backport.patch)：四个 bit reader 的 16 处移位先转换为 unsigned long；framing 的字段访问移至状态检查之后，并包含原补丁的 self-test 转换

修补前的实际 sanitizer 在完整 libvorbisfile 读回时触发 `bitwise.c:398` 的有符号移位错误，原结果保留；这项修复已进入 libogg v1.3.6，但截至 2026-10-07 最新稳定 sys 归档仍未包含它，故通过根 `[patch.crates-io]` 使用本地副本，原版本和接口保持不变

升级 sys 时核对官方归档是否已有修复，重新执行完整回读与 sanitizer，保留 COPYING、Rust binding LICENSE 和源码 notice；原错误、修补后的软件及跨目标结果分别记录，未观察到诊断不代表全输入域形式安全证明
