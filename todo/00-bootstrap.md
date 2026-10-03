# 00 · 工程底座

- [x] 只创建 game、schema、core、runtime、replay、lab、xtask。
- [x] 使用最新 stable Rust、Edition 2024，提交 Cargo.lock；新依赖选最新稳定版。
- [x] 明确 MPL-2.0，建立第三方与资源来源台账。
- [x] 实现 SongTime、MusicalTick、SessionEpoch 与检查溢出的时间转换。
- [x] 实现依赖边界检查、doctor、fmt / Clippy / workspace test 命令。
- [x] 配置轻量 Linux 自动 CI 和 Windows MSVC / Linux 的 x86-64 / ARM64 手动构建。
- [x] 配置以运行性能为目标的 release profile。
- [x] 工程底座的本地 Linux 检查通过，包括十小时整数时间累计
- [x] 已只读验证底座 `b2950cc` 的首次 [Lightweight CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/36963977703) 为 PASS：fmt、依赖边界及 schema/core/replay/xtask 的 Clippy 与测试步骤成功
- [x] 集成原型的 [Lightweight CI 37083062753](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37083062753) 在 `0340ea9` 通过；后续代码须取得各自的 CI 记录
- [x] 四目标原生 release 构建及下载包静态核验通过：Linux 两架构为 `4c63dcc`，Windows 两架构在 `34eaf5b` 修复换行转换后通过；首轮 Windows 包失败保留，见 [构建记录](../docs/build-release.md#2026-10-03-四目标原生-runner-构建)

新增 Bevy/Kira 运行层的当前集成证据见 [验证策略](../docs/testing.md)，不沿用底座检查结果推断新功能通过

退出条件：底座可编译、检查可执行、时间测试通过，可以推进原型与设备测量；远端 CI、四目标构建和真实兼容性分别记录

本地新源码不能继承旧版本的 CI 通过结论，须取得对应提交的运行证据

版本 tag 自动发布另见 [发布流程](../docs/build-release.md#推送版本-tag-自动发布)：`Vx.y.z` 与 Cargo 一致后执行 CI、四目标构建和四份压缩包发布；本地工作流检查通过，首次真实 tag 发布单独取证
