# 00 · 工程底座

- [x] 只创建 game、schema、core、runtime、replay、lab、xtask。
- [x] 使用最新 stable Rust、Edition 2024，提交 Cargo.lock；新依赖选最新稳定版。
- [x] 明确 MPL-2.0，建立第三方与资源来源台账。
- [x] 实现 SongTime、MusicalTick、SessionEpoch 与检查溢出的时间转换。
- [x] 实现依赖边界检查、doctor、fmt / Clippy / workspace test 命令。
- [x] 配置轻量 Linux 自动 CI 和 Windows MSVC / Linux 的 x86-64 / ARM64 手动构建。
- [x] 配置以运行性能为目标的 release profile。
- [x] 工程底座的本地 Linux 检查通过，包括十小时整数时间累计
- [ ] 在 GitHub 验证轻量 CI 与四种目标的手动构建首次运行结果。

新增 Bevy/Kira 运行层的当前集成证据见 [验证策略](../docs/testing.md)，不沿用底座检查结果推断新功能通过

退出条件：底座可编译、检查可执行、时间测试通过，可以推进原型与设备测量；远端 CI、四目标构建和真实兼容性分别记录
