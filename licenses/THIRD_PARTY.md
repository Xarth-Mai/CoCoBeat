# 第三方依赖台账

项目源代码使用 [MPL-2.0](../LICENSE)。下表根据当前 `Cargo.lock` 与 `cargo metadata --locked --format-version 1` 整理（2026-10-02），记录解析图中的第三方包，包含可能未激活的可选依赖；不等同于最终发行二进制清单。

Day 0 唯一直接第三方 Rust 依赖是 **xtask 使用的 serde_json**，用于检查 Cargo metadata。游戏、schema、core、replay 当前未引入外部 Rust 包。此清单中的许可证是上游 manifest 声明；实际分发时按所使用组件保留对应许可文本与 notices。

| 包 | 锁定版本 | 上游声明许可证 |
|---|---|---|
| [itoa](https://crates.io/crates/itoa/1.0.18) | 1.0.18 | MIT OR Apache-2.0 |
| [memchr](https://crates.io/crates/memchr/2.8.3) | 2.8.3 | Unlicense OR MIT |
| [proc-macro2](https://crates.io/crates/proc-macro2/1.0.107) | 1.0.107 | MIT OR Apache-2.0 |
| [quote](https://crates.io/crates/quote/1.0.47) | 1.0.47 | MIT OR Apache-2.0 |
| [serde](https://crates.io/crates/serde/1.0.229) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_core](https://crates.io/crates/serde_core/1.0.229) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_derive](https://crates.io/crates/serde_derive/1.0.229) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_json](https://crates.io/crates/serde_json/1.0.151) | 1.0.151 | MIT OR Apache-2.0 |
| [syn](https://crates.io/crates/syn/3.0.6) | 3.0.6 | MIT OR Apache-2.0 |
| [unicode-ident](https://crates.io/crates/unicode-ident/1.0.26) | 1.0.26 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| [zmij](https://crates.io/crates/zmij/1.0.23) | 1.0.23 | MIT |

新增依赖采用当时最新稳定版本，更新 Cargo.lock 后同步本表。接入 Bevy、Kira、Symphonia、OxiMedia、Quinn 时重新核对真实许可证与依赖图；研究报告中的库选择不构成已接入或已审查的证明。

工作流使用 `actions/checkout v7.0.1`、`actions/cache v6.1.0`、`actions/upload-artifact v7.0.1`（均 MIT），不属于 Cargo 解析图或游戏运行时依赖。素材与音乐单独记录在 [ASSET_PROVENANCE.csv](ASSET_PROVENANCE.csv)，当前无资源条目。
