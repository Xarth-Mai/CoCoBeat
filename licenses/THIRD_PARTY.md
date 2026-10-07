# 第三方依赖台账

项目源代码使用 [MPL-2.0](../LICENSE)。本台账于 2026-10-07 根据 `cargo metadata --offline --locked --format-version 1` 整理，完整解析图见 [THIRD_PARTY.csv](THIRD_PARTY.csv)，共 589 个第三方包，与当前 `Cargo.lock` 一致，包含未激活平台及可选依赖，含本地回移官方修复的 sys 第三方副本，不等同于最终发行二进制清单

| 直接依赖 | 锁定版本 | 使用方与用途 | 上游声明许可证 |
|---|---|---|---|
| [bevy](https://github.com/bevyengine/bevy) | 0.19.1 | runtime：窗口、输入、3D 与 UI；lab：原生时间线工作台 | MIT OR Apache-2.0 |
| [kira](https://github.com/tesselode/kira) | 0.12.5 | runtime：音乐与即时反馈音频 | MIT OR Apache-2.0 |
| [fontique](https://github.com/linebender/parley) | 0.9.0 | runtime：复用 Bevy 已解析的字体集合，为嵌入 Noto 配置原生跨脚本回退 | Apache-2.0 OR MIT |
| [symphonia](https://github.com/pdeljanov/Symphonia) | 0.6.1 | media：有上限的源音频解码，以及 runtime 歌曲包的严格 Ogg Vorbis 读回 | MPL-2.0 |
| [oximedia-audio](https://github.com/cool-japan/oximedia) | 0.2.1 | media：High 窗化 sinc 重采样，关闭默认 codec features | Apache-2.0 |
| [oximedia-core](https://github.com/cool-japan/oximedia) | 0.2.1 | media：重采样适配器内部的 PCM 格式 | Apache-2.0 |
| [vorbis_rs](https://github.com/ComunidadAylas/vorbis-rs) | 0.5.6 | media：静态内嵌 aoTuV/Lancer Vorbis 编码候选，关闭默认 RNG feature，sys 回移三项官方修补并恢复 libogg 位打包，软件准入独立验证 | BSD-3-Clause |
| [blake3](https://github.com/BLAKE3-team/BLAKE3) | 1.8.7 | media：内容对象身份；net：证书、模板和实际 Replay 的字节哈希 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception |
| [serde](https://github.com/serde-rs/serde) | 1.0.229 | replay：事实序列化；runtime：设置持久化；media/lab：私有内容格式和创作输入；net：邀请、严格消息与摘要 | MIT OR Apache-2.0 |
| [postcard](https://github.com/jamesmunns/postcard) | 1.1.3 | media：有界且带独立版本头的内容对象编码，schema 保持标准库类型 | MIT OR Apache-2.0 |
| [serde_json](https://github.com/serde-rs/json) | 1.0.151 | replay/runtime/lab/net：JSON 编解码；xtask：Cargo metadata 检查 | MIT OR Apache-2.0 |
| [winit](https://github.com/rust-windowing/winit) | 0.30.13 | runtime：窗口图标，复用 Bevy 已启用的平台功能 | Apache-2.0 |
| [sys-locale](https://github.com/1Password/sys-locale) | 0.3.2 | runtime：Windows/Linux 系统首选语言识别 | MIT OR Apache-2.0 |
| [winsafe](https://github.com/rodrigocfd/winsafe) | 0.0.29 | runtime：Windows 显示器工作区的安全原生接口 | MIT |
| [x11rb](https://github.com/psychon/x11rb) | 0.14.0 | runtime：Linux X11 工作区与窗口调整权限读取 | MIT OR Apache-2.0 |
| [quinn](https://github.com/quinn-rs/quinn) | 0.11.12 | net：可靠 QUIC 会话与标准 TLS 客户端 | MIT OR Apache-2.0 |
| [rcgen](https://github.com/rustls/rcgen) | 0.14.10 | net：每次会话自签证书 | MIT OR Apache-2.0 |
| [tokio](https://github.com/tokio-rs/tokio) | 1.53.1 | net：单线程网络 runtime、有界队列与超时 | MIT |
| [embed-resource](https://github.com/nabijaczleweli/rust-embed-resource) | 3.0.11 | game：仅 Windows 构建时嵌入 EXE 图标 | MIT |

Vorbis 路径的 Rust binding BSD、Vorbis / libogg COPYING 以及编译路径中的 LPC 独立 notice 原文与实际归档身份保留在 [vorbis-rs 来源目录](vorbis-rs/README.md)，随现有 licenses 目录进入发行包；Vorbis sys 四文件修补和 libogg sys 两文件官方修补及原归档身份分别见 [Vorbis UPSTREAM](../vendor/aotuv_lancer_vorbis_sys/UPSTREAM.md) 与 [libogg UPSTREAM](../vendor/ogg_next_sys/UPSTREAM.md)；完整文本收录不替代编码器及四目标发行验证

CSV 按包名和版本记录上游 manifest 的 `name`、`version`、`license`、`repository`，缺失的 repository 保留空值。声明许可证不代表发行许可审查已通过；发行前需核对实际分发组件及资源，准备适用的许可文本与 notices

独立音频和 MIR 研究工具的依赖另见 [CANONICAL_PROBE_DEPENDENCIES.csv](CANONICAL_PROBE_DEPENDENCIES.csv)：2026-10-03 对 `tools/canonical-audio-probe/` 的五个工具包、`tools/mir-onset-probe/`、`tools/mir-onset-diagnostic/`、`tools/mir-spectral-probe/`、`tools/mir-flux-probe/` 与 `tools/mir-flux-gate-probe/` 分别执行 `cargo metadata --offline --locked --format-version 1 --manifest-path <工具包>/Cargo.toml`，按 name/version 合并得到 88 个第三方包，`used_by` 记录使用包；各解析闭包与独立 Cargo.lock 一致，五个 MIR 工具分别解析相同的 37 个第三方包，使用 Apache-2.0 的 OxiMedia MIR 0.2.1，谱候选另将已有的 Apache-2.0 `oxifft 0.4.2` 列为直接依赖并开启 `std`、`streaming`，第三方包版本集合不变；产品未启用该分析库，该研究台账不扩充产品 Cargo.lock 或发行组件清单

`cocobeat-rusty-candidate` 的独立闭包包含 64 个 registry 包和一个 patched vendor。`rusty_vorbis 0.1.1` 的 name/version 台账记录许可元数据，其发布版与修补版的代码身份分别保存；候选 vendor 从官方 crate archive 提取，仅修改 `forward_couple`，完整 Apache-2.0 LICENSE、原 README 与上游说明保留，来源、14 个文件身份与唯一补丁见 [UPSTREAM.md](../tools/canonical-audio-probe/rusty-candidate/UPSTREAM.md)。该副本用于复现编码候选，未进入产品或游戏发行包

依赖采用最新稳定版本，manifest 使用主版本范围；更新 `Cargo.lock` 后同步 CSV 和直接依赖表，精确版本用于记录实际解析结果

工作流使用 `actions/checkout@v7`、`actions/cache@v6`、`actions/upload-artifact@v7`、`actions/download-artifact@v8`（均 MIT），跟随各主版本的稳定更新，不属于 Cargo 解析图或游戏运行时依赖

原创开发音乐、Anchor、独立事件帧标注及其手工包创作 JSON、反馈音、探针脉冲与品牌落点合成音的来源见 [ASSET_PROVENANCE.csv](ASSET_PROVENANCE.csv)，这些生成资源使用 CC0-1.0，生成器源代码使用 MPL-2.0。`source_hash` 使用 SHA-256，反馈音、探针脉冲与品牌落点生成器条目记录源码哈希，其他条目记录资源文件哈希；听感人工验收与真实设备音频验收均为 NOT RUN

MIR 原创脉冲、静默和独立字面帧号真值也使用 CC0-1.0，`mir-onset-clean-v1` 记录生成器源码哈希，源码使用 MPL-2.0；后续 `mir-onset-holdout-v1` 记录相位扫描、短尾、持续音与固定种子噪声的生成器身份，`mir-spectral-frequency-check-v1` 记录原创 1500 Hz 与 7500 Hz 同能量正弦的频谱自检生成器身份，`mir-flux-declared-controls-v1` 记录预先声明的静默后起音、等能量换音、稀疏打击与连续渐变生成器身份，PCM 和构造标签同为 CC0-1.0；这些标签表示构造攻击的位置，不能作为 beat 或人工 Anchor 标签

全频带诊断的 `canonical-fullband-controls-v1` 记录低音量、声道对称与边界脉冲生成器的源码哈希；生成 PCM 使用 CC0-1.0，脚本使用 MPL-2.0，复用的旧样本保留原来源，不作为听感或生产准入证据

源导入回归的 `mono.mp3`、`mono.ogg` 与 `stereo-canonical.ogg` 均是原创 CC0-1.0 合成正弦，资源台账记录各文件 SHA-256，精确生成命令见 [样本说明](../testdata/synthetic/media-import/README.md)；FFmpeg 及其编码器仅用于开发期生成独立样本，不进入测试执行环境或产品依赖

品牌的 18 条来源记录从 [assets/brand/PROVENANCE.csv](../assets/brand/PROVENANCE.csv) 原样并入资源总台账。用户提供的字标与 Symbol 参考图未附原作者信息及原始再分发授权，相关条目保留 `UNSPECIFIED_REFERENCE` 与 `unverified`；项目代码许可证与这些品牌参考图的权利信息分别记录，本轮未核验其对外再分发许可

界面字体采用六份未经修改的官方 Noto Sans 字体：基础 Noto Sans 2.015 与 Noto Sans CJK 2.004 的 SC、TC、HK、JP、KR 地区字体，共覆盖首批 13 个 locale 的字体选择；保留各 locale 首选字形，缺字时在现有字集中按脚本回退，未承诺任意 Unicode；逐文件来源、固定提交、SHA-256、内嵌版权和语言映射见 [字体来源清单](../assets/fonts/SOURCES.json)，六条字体记录已并入资源总台账，发行包保留原始 [Noto Sans OFL](../assets/fonts/NotoSans-OFL.txt) 与 [Noto Sans CJK OFL](../assets/fonts/NotoSansCJK-OFL.txt) 及版权声明，均使用 SIL Open Font License 1.1

语言选择器的 13 组旗帜来自 [flag-icons v7.5.0](https://github.com/lipis/flag-icons/releases/tag/v7.5.0)，原始 4:3 SVG 与派生 96×72 PNG 的 26 条记录已并入资源总台账；固定提交、原件和派生文件 SHA-256、导出方式见 [旗帜来源清单](../assets/flags/SOURCES.json)，发行包保留上游 [MIT 许可和版权声明](../assets/flags/LICENSE)，这些资源不新增游戏运行时依赖
