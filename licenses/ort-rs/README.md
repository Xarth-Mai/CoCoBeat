# ORT Rust wrapper 原始许可

本目录保留当前 Cargo.lock 新解析六包与既有 cfg-if / smallvec / windows-link wrapper 依赖的官方 registry 许可原文，文件均与该 crate 原归档逐字节一致；来源身份见本次开发核验记录，name/version/license/repository 同步在 [第三方总台账](../THIRD_PARTY.csv)

MIT OR Apache-2.0 的包可选择 MIT 条款，原双许可文本均保留；历史 `MIT/Apache-2.0` 声明保持 manifest 原文，libloading 使用 ISC，原 copyright 和 permission 文本不以项目 MPL 替代

这份目录同时覆盖新 lock 中的可选 ndarray / matrixmultiply / rawpointer 解析条目，不表示它们在所有平台或当前 wrapper 配置实际链接；产品当前只使用安全 ORT 动态 CPU SDK，SDK 原始 notices 与 Eigen 源码说明位于 [onnxruntime](../onnxruntime/SOURCE-AVAILABILITY.md)，模型和 minimal 移植的 MIT 位于 [beat-this/LICENSE](../beat-this/LICENSE)

| 包与当前锁定版本 | 原始许可文件 |
|---|---|
| cfg-if 1.0.5 | [LICENSE-APACHE](cfg-if-1.0.5/LICENSE-APACHE) / [LICENSE-MIT](cfg-if-1.0.5/LICENSE-MIT) |
| libloading 0.9.0 | [LICENSE](libloading-0.9.0/LICENSE) |
| matrixmultiply 0.3.11 | [LICENSE-APACHE](matrixmultiply-0.3.11/LICENSE-APACHE) / [LICENSE-MIT](matrixmultiply-0.3.11/LICENSE-MIT) |
| ndarray 0.17.2 | [LICENSE-APACHE](ndarray-0.17.2/LICENSE-APACHE) / [LICENSE-MIT](ndarray-0.17.2/LICENSE-MIT) |
| ort 2.0.0-rc.13 | [LICENSE-APACHE](ort-2.0.0-rc.13/LICENSE-APACHE) / [LICENSE-MIT](ort-2.0.0-rc.13/LICENSE-MIT) |
| ort-sys 2.0.0-rc.13 | [LICENSE-APACHE](ort-sys-2.0.0-rc.13/LICENSE-APACHE) / [LICENSE-MIT](ort-sys-2.0.0-rc.13/LICENSE-MIT) |
| rawpointer 0.2.1 | [LICENSE-APACHE](rawpointer-0.2.1/LICENSE-APACHE) / [LICENSE-MIT](rawpointer-0.2.1/LICENSE-MIT) |
| smallvec 1.16.2 | [LICENSE-APACHE](smallvec-1.16.2/LICENSE-APACHE) / [LICENSE-MIT](smallvec-1.16.2/LICENSE-MIT) |
| windows-link 0.2.1 | [license-apache-2.0](windows-link-0.2.1/license-apache-2.0) / [license-mit](windows-link-0.2.1/license-mit) |
