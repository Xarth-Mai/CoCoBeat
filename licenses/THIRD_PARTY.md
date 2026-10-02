# 第三方依赖台账

项目源代码使用 [MPL-2.0](../LICENSE)。本台账于 2026-10-02 根据 `cargo metadata --offline --locked --format-version 1` 整理，完整解析图见 [THIRD_PARTY.csv](THIRD_PARTY.csv)，共 516 个第三方包，与当前 `Cargo.lock` 一致，包含未激活平台及可选依赖，不等同于最终发行二进制清单

| 直接依赖 | 锁定版本 | 使用方与用途 | 上游声明许可证 |
|---|---|---|---|
| [bevy](https://github.com/bevyengine/bevy) | 0.19.1 | runtime：窗口、输入、3D 与 UI | MIT OR Apache-2.0 |
| [kira](https://github.com/tesselode/kira) | 0.12.5 | runtime：音乐与即时反馈音频 | MIT OR Apache-2.0 |
| [serde](https://github.com/serde-rs/serde) | 1.0.229 | replay：事实序列化；runtime：设置持久化 | MIT OR Apache-2.0 |
| [serde_json](https://github.com/serde-rs/json) | 1.0.151 | replay/runtime：JSON 编解码；xtask：Cargo metadata 检查 | MIT OR Apache-2.0 |
| [winit](https://github.com/rust-windowing/winit) | 0.30.13 | runtime：窗口图标，复用 Bevy 已启用的平台功能 | Apache-2.0 |
| [sys-locale](https://github.com/1Password/sys-locale) | 0.3.2 | runtime：Windows/Linux 系统首选语言识别 | MIT OR Apache-2.0 |
| [embed-resource](https://github.com/nabijaczleweli/rust-embed-resource) | 3.0.11 | game：仅 Windows 构建时嵌入 EXE 图标 | MIT |

CSV 按包名和版本记录上游 manifest 的 `name`、`version`、`license`、`repository`，缺失的 repository 保留空值。声明许可证不代表发行许可审查已通过；发行前需核对实际分发组件及资源，准备适用的许可文本与 notices

独立 canonical 音频研究工具的依赖另见 [CANONICAL_PROBE_DEPENDENCIES.csv](CANONICAL_PROBE_DEPENDENCIES.csv)：2026-10-02 对 `tools/canonical-audio-probe/` 的四个工具包分别执行 `cargo metadata --offline --locked --format-version 1 --manifest-path <工具包>/Cargo.toml`，按 name/version 合并得到 74 个第三方包，`used_by` 记录使用包；各解析闭包与独立 Cargo.lock 一致，发布归档 SHA-256 均匹配锁文件校验和，该研究台账不扩充产品 Cargo.lock 或发行组件清单

依赖采用最新稳定版本，manifest 使用主版本范围；更新 `Cargo.lock` 后同步 CSV 和直接依赖表，精确版本用于记录实际解析结果

工作流使用 `actions/checkout@v7`、`actions/cache@v6`、`actions/upload-artifact@v7`（均 MIT），跟随各主版本的稳定更新，不属于 Cargo 解析图或游戏运行时依赖

原创开发音乐、Anchor、独立事件帧标注、反馈音、探针脉冲与品牌落点合成音的来源见 [ASSET_PROVENANCE.csv](ASSET_PROVENANCE.csv)，这些生成资源使用 CC0-1.0，生成器源代码使用 MPL-2.0。`source_hash` 使用 SHA-256，反馈音、探针脉冲与品牌落点生成器条目记录源码哈希，其他条目记录资源文件哈希；听感人工验收与真实设备音频验收均为 NOT RUN

品牌的 18 条来源记录从 [assets/brand/PROVENANCE.csv](../assets/brand/PROVENANCE.csv) 原样并入资源总台账。用户提供的字标与 Symbol 参考图未附原作者信息及原始再分发授权，相关条目保留 `UNSPECIFIED_REFERENCE` 与 `unverified`；项目代码许可证与这些品牌参考图的权利信息分别记录，本轮未核验其对外再分发许可

界面字体采用六份未经修改的官方 Noto Sans 字体：基础 Noto Sans 2.015 与 Noto Sans CJK 2.004 的 SC、TC、HK、JP、KR 地区字体，共覆盖首批 13 个 locale 的字体选择；逐文件来源、固定提交、SHA-256、内嵌版权和语言映射见 [字体来源清单](../assets/fonts/SOURCES.json)，六条字体记录已并入资源总台账，发行包保留原始 [Noto Sans OFL](../assets/fonts/NotoSans-OFL.txt) 与 [Noto Sans CJK OFL](../assets/fonts/NotoSansCJK-OFL.txt) 及版权声明，均使用 SIL Open Font License 1.1

语言选择器的 13 组旗帜来自 [flag-icons v7.5.0](https://github.com/lipis/flag-icons/releases/tag/v7.5.0)，原始 4:3 SVG 与派生 96×72 PNG 的 26 条记录已并入资源总台账；固定提交、原件和派生文件 SHA-256、导出方式见 [旗帜来源清单](../assets/flags/SOURCES.json)，发行包保留上游 [MIT 许可和版权声明](../assets/flags/LICENSE)，这些资源不新增游戏运行时依赖
