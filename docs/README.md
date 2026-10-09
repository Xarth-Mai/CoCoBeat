# 文档索引

这些文档记录产品约束、当前实现与验收边界；研究报告是设计来源，其中的外部结论、兼容性和性能数字不等于本项目的实验证据

当前已实现本地 64 秒双人原型，软件检查与真实设备/体验验收分开记录在 [testing.md](testing.md) 和 [工作进度](../todo/progress.md)；项目采用 MPL-2.0，Rust 与依赖使用最新稳定版，具体版本以工具链记录和 Cargo.lock 为准

| 文档 | 回答的问题 |
|---|---|
| [product.md](product.md) | 做什么、不做什么，如何判断双人体验成立 |
| [architecture.md](architecture.md) | 谁拥有事实、规则与实现，何时新增 crate |
| [timing.md](timing.md) | 音乐时间的单位、边界和待测量的不确定性 |
| [gameplay.md](gameplay.md) | 已实现规则、初始参数与待验证的体验 |
| [editor.md](editor.md) | 原生波形、歌曲试听、Replay / 候选证据与精确 Anchor 编辑 |
| [independent-labels.md](independent-labels.md) | 来源绑定的人工标签导入、双人分歧与验收边界 |
| [source-import.md](source-import.md) | 有界源快照、唯一生产编码器、手工内容四对象导入 |
| [native-beat-candidate.md](native-beat-candidate.md) | 显式原生 beat / downbeat 候选、固定资源及未准入边界 |
| [native-structure-features.md](native-structure-features.md) | 最终音频的谱形变化、相似邻居、真实 EOF 与未准入音乐语义 |
| [network-sessions.md](network-sessions.md) | 邀请、四对象接收与 Ready、QUIC 可靠历史、断线前缀与应用完成确认 |
| [visual-polish.md](visual-polish.md) | 原创角色、循环街区、原生光影与落点反馈的画面及验证 |
| [testing.md](testing.md) | 自动检查、硬件实验和真人测试各自证明什么 |
| [canonical-audio-probe.md](canonical-audio-probe.md) | 编码候选的失败、适配与有限回读证据，以及复现入口 |
| [build-release.md](build-release.md) | 性能优化发行参数、轻量 CI 与四种目标的手动构建 |
| [platform-input.md](platform-input.md) | Windows/Linux 与初版手柄支持的实现边界和验收 |
| [assets-licenses.md](assets-licenses.md) | 哪些资源可以进入仓库和发行包 |
| [decisions/ADR-0001-bootstrap.md](decisions/ADR-0001-bootstrap.md) | 工程底座的原始范围、许可证和版本策略 |
| [../todo/README.md](../todo/README.md) | 按依赖与验收门槛推进的工作顺序 |

新增专题文档应承载实际设计或实验结果；开发音乐见 [资源说明](../assets/dev/vertical_slice/README.md)，软件计时模拟见 [实验说明](../testdata/synthetic/README.md)

- [SongPackage](song-package.md)：初始四对象格式、手工创作入口、完整验证与原子发布
