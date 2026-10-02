# 验证策略

## 当前自动检查

```sh
cargo xtask doctor
cargo xtask check
cargo run --locked -p cocobeat-lab -- time-smoke
cargo run --locked -p cocobeat-game
```

`doctor` 检查 Rust/Cargo/rustfmt/Clippy 与依赖边界；`check` 检查边界、格式、Clippy 和整个 workspace 的测试。命令失败会返回非零状态，未知子命令也不会假装成功。

SongTime 测试覆盖整数单位、负预滚、半帧舍入、非有限数、范围边界、算术溢出和十小时音频块累计。时间预期值使用独立字面常量，避免生成器和验证器共享错误。
边界测试覆盖反向依赖、重命名的平台 dev 依赖和未知本地 helper。

自动 CI 使用单个 Linux job 和提交的 lockfile：全仓库格式、依赖边界，以及 schema/core/replay/xtask 的 Clippy 与测试。使用缓存、路径过滤和旧任务取消；不自动编译 runtime/game、跑平台矩阵或制作 release。
本地 `cargo xtask check` 仍执行完整 workspace 检查。Windows/Linux release 使用手动 Action，各自可选 x86-64 / ARM64，默认 Windows x86-64 MSVC。完整平台与真实设备验收仍是发行门槛，轻量 CI 不能替代，见 [构建与 CI](build-release.md)。
只改文档时工作流会被跳过；如果配置 GitHub 必需状态检查，不应让被路径过滤跳过的检查阻塞文档 PR。

## 下一阶段必须新增的证据

- 核心：一对一匹配、重复输入、阈值相等、进度水位、epoch、Anchor/Free 去重与有界 Resonance。
- Replay：同一事实在不同批次、60/144 Hz 消费和允许的交付重排下得到相同最终结果。
- 硬件：真实输入与音频输出延迟、ClockBridge 偏移/漂移；软件单元测试不能代替。
- 体验：玩家对同伴、沉默和共同反馈的具体行为与访谈，不能用算法分数代替。
- 平台与手柄：Windows/Linux 各自完整构建、运行与音频验证；双手柄、混合输入、菜单、重绑定、断连/重连与独立延迟测量，见 [验收矩阵](platform-input.md)。

之后再增加编码回读、资源事务/哈希、MIR 标注、QUIC 模拟和两台真实机器测试。未实现这些能力前，不提供始终成功的 validate-testdata 或 package-golden 命令。

每项功能完成时说明：责任、真值来源、失败行为、成功测试、可重放证据，以及是否引入第二条正式实现路径。
