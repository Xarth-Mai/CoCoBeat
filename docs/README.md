# 文档索引

这些文档把用户提供的研究报告与本次仓库决策整理为可执行约束。报告中的外部研究结论、库兼容性和性能数字仍需独立验证，不能作为已完成的实验结果。
本次明确覆盖报告的两项建议：项目采用 MPL-2.0；Rust 与新增依赖使用最新稳定版。

| 文档 | 回答的问题 |
|---|---|
| [product.md](product.md) | 做什么、不做什么，如何判断双人体验成立 |
| [architecture.md](architecture.md) | 谁拥有事实、规则与实现，何时新增 crate |
| [timing.md](timing.md) | 音乐时间的单位、边界和待测量的不确定性 |
| [gameplay.md](gameplay.md) | Free Sync、Anchor Sync、反馈的语义约束 |
| [testing.md](testing.md) | 自动检查、硬件实验和真人测试各自证明什么 |
| [build-release.md](build-release.md) | 性能优化发行参数、轻量 CI 与四种目标的手动构建 |
| [platform-input.md](platform-input.md) | Windows/Linux 与初版手柄支持的实现边界和验收 |
| [assets-licenses.md](assets-licenses.md) | 哪些资源可以进入仓库和发行包 |
| [decisions/ADR-0001-bootstrap.md](decisions/ADR-0001-bootstrap.md) | Day 0 范围、许可证和版本策略 |
| [../todo/README.md](../todo/README.md) | 按依赖与验收门槛推进的工作顺序 |

audio、MIR、stage、network 等专题文档在相应里程碑中产生真实设计和实验结果时添加，不提前建立空文档。
