# ADR-0001：最小 workspace 与工程基线

状态：已采用（2026-10-02）。

## 背景

最终架构包括音乐理解、舞台编译、联网和编辑器，但当前最重要的不确定性是两个人能否从稀疏 Anchor 与自由演奏中获得共同体验。

## 决策

只建立 game、schema、core、runtime、replay、lab 与 xtask。当前实现时间契约和工程检查，其余能力按实际消费关系逐步落地，不生成最终目录树中的所有空模块。
schema/core 隔离第三方实现库；规则输出语义事实，表现不反向影响规则。新增 crate 必须拥有独立责任和验收条件。

用户明确确定 MPL-2.0，保留仓库既有 LICENSE。Rust 跟随最新 stable，依赖采用最新稳定版；Cargo manifest 使用主版本范围，GitHub Actions 使用最新稳定主版本标签。Cargo.lock 固定实际解析版本，研究报告中的版本号只作为历史参考

## 代价与约束

stable 会随时间升级，历史工具链复现能力弱于固定版本；实验报告必须记录 `rustc --version`、代码提交和 Cargo.lock。
Cargo 依赖在声明范围内通过 `cargo update` 更新；跨主版本时，需要修改 manifest、迁移 API 并验证。未来 Bevy/Kira 的接入必须测试实际兼容性，不把报告的版本表视为兼容性证明
