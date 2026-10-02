# 第三方依赖台账

项目源代码使用 [MPL-2.0](../LICENSE)。本台账于 2026-10-02 根据 `cargo metadata --offline --locked --format-version 1` 整理，完整解析图见 [THIRD_PARTY.csv](THIRD_PARTY.csv)，共 516 个第三方包，与当前 `Cargo.lock` 一致，包含未激活平台及可选依赖，不等同于最终发行二进制清单

| 直接依赖 | 锁定版本 | 使用方与用途 | 上游声明许可证 |
|---|---|---|---|
| [bevy](https://github.com/bevyengine/bevy) | 0.19.1 | runtime：窗口、输入、3D 与 UI | MIT OR Apache-2.0 |
| [kira](https://github.com/tesselode/kira) | 0.12.5 | runtime：音乐与即时反馈音频 | MIT OR Apache-2.0 |
| [serde](https://github.com/serde-rs/serde) | 1.0.229 | replay：事实序列化 | MIT OR Apache-2.0 |
| [serde_json](https://github.com/serde-rs/json) | 1.0.151 | replay：JSON 编解码；xtask：Cargo metadata 检查 | MIT OR Apache-2.0 |
| [winit](https://github.com/rust-windowing/winit) | 0.30.13 | runtime：窗口图标，复用 Bevy 已启用的平台功能 | Apache-2.0 |
| [embed-resource](https://github.com/nabijaczleweli/rust-embed-resource) | 3.0.11 | game：仅 Windows 构建时嵌入 EXE 图标 | MIT |

CSV 按包名和版本记录上游 manifest 的 `name`、`version`、`license`、`repository`，缺失的 repository 保留空值。声明许可证不代表发行许可审查已通过；发行前需核对实际分发组件及资源，准备适用的许可文本与 notices

依赖采用最新稳定版本，manifest 使用主版本范围；更新 `Cargo.lock` 后同步 CSV 和直接依赖表，精确版本用于记录实际解析结果

工作流使用 `actions/checkout@v7`、`actions/cache@v6`、`actions/upload-artifact@v7`（均 MIT），跟随各主版本的稳定更新，不属于 Cargo 解析图或游戏运行时依赖

原创开发音乐、Anchor、独立事件帧标注、反馈音、探针脉冲与品牌落点合成音的来源见 [ASSET_PROVENANCE.csv](ASSET_PROVENANCE.csv)，这些生成资源使用 CC0-1.0，生成器源代码使用 MPL-2.0。`source_hash` 使用 SHA-256，反馈音、探针脉冲与品牌落点生成器条目记录源码哈希，其他条目记录资源文件哈希；听感人工验收与真实设备音频验收均为 NOT RUN

品牌的 18 条来源记录从 [assets/brand/PROVENANCE.csv](../assets/brand/PROVENANCE.csv) 原样并入资源总台账。用户提供的字标与 Symbol 参考图未附原作者信息及原始再分发授权，相关条目保留 `UNSPECIFIED_REFERENCE` 与 `unverified`；项目代码许可证与这些品牌参考图的权利信息分别记录，本轮未核验其对外再分发许可
