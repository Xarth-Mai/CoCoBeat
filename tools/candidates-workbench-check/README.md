# Anchor 候选工作台受控验证

复用 Replay 检查脚本的哈希与 owned process group 清理，复制同一组七个生产模块，仅在工作台副本注入窗口尺寸、合成键盘驱动与 Bevy 原生截图；每次使用新的证据目录，脚本不会调用 Cargo

```sh
python3 tools/candidates-workbench-check/check.py prepare NEW_EVIDENCE_DIR
python3 tools/candidates-workbench-check/check.py build NEW_EVIDENCE_DIR RECORDED_CARGO_JSON SUCCESSFUL_LAB_BUILD_RECORD
python3 tools/candidates-workbench-check/check.py run NEW_EVIDENCE_DIR
```

`build` 核对成功依赖记录中的 lab 源码、13 份 catalog 和确切 extern 路径及 SHA-256，随后仅用 `rustc` 链接这些批准的 debug 产物；后续 C 源码修改和整个当前仓库不在这个编译声明中，系统 native 库沿用环境，不通过本检查验收

最终运行的依赖记录为 `target/candidates-workbench-20261007/current-build.jsonl` 和 `readable-tests/build-result.json`；复现须提供与当前源码及产物哈希一致的成功记录，后续 Cargo 构建可能覆盖旧 debug 产物，旧记录不会自动批准新产物

helper 的 `--fixture NEW_DIRECTORY` 使用正式包构建器和候选编译器，生成 rich 与 empty 两套四对象资源及报告；音频 RMS 和 peak 来自最终 canonical PCM，onset、beat、section 是明确标注的未校准机制控制，不构成 MIR 准入证据

`run` 在独立 headless gamescope 中检查 rich 的 1280×800、640×480 和 empty 的 640×480；输入通过生产 capture，五张 rich 截图分别保留未知评分、密集拒绝及 blocker、提案与源 chart 分离、列表尾部和详情底部，empty 保存零候选状态；包及报告、生产副本输入、catalog、工具、确切 extern 和项目 native archive 的哈希在运行前后保持一致

`prepared.json`、`instrumentation.patch`、`build.json`、`fixture.json`、`run.json` 和逐进程日志记录命令、错误、超时、退出码及截图 SHA-256；只有 Driver 完成所有只读与显示断言、截图集合齐全且输入未变才通过软件检查，PNG 仍需视觉复核，物理控制器、音频与真人体验另行验收

2026-10-07 最终 `gui-readable` 三组原生 GPU 检查通过：rich 的两个尺寸各浏览 40 条候选，empty 核对零候选，合计注入 240 / 252 / 18 条键盘消息并保存 11 张 PNG；全部截图经 agent 和主线程分别目检通过，28 项实际 lab 测试及 13 份各 206 key 的 catalog／占位符检查通过，冻结源码、依赖和原始记录见[持久观察清单](../../testdata/synthetic/candidates-workbench-observations-20261007.json)

主线程随后实际运行 `cargo test -p cocobeat-lab --locked`、lab 的 `cargo clippy --all-targets --locked -- -D warnings`、全仓格式与依赖边界检查，均通过；精确命令和日志另列于观察清单的 `supplemental_current_workspace_checks`，当时工作区包含未提交的 Stage 新 API，这些检查与此前冻结 GUI 的产物身份分别记录

此前的旧 catalog 两项测试失败、QA 浮点类型不一致、Wayland socket 路径过长和自动断言通过但选中证据在首屏之外的视觉失败独立保留；最终修复把选中证据放在详情顶部，原事实及原失败记录保持，`run.json` 仍记录当时未目检，后续目检结论以单独 `visual-review.json` 为准

该批链接的 debug Stage 产物包含当时未提交的 `compile_version`／版本 getter；候选工作台只调用既有默认编译行为，不调用新增入口，此证据不证明整个提交的 Cargo 构建或历史 Stage 画面复现。观察清单记录运行时 README 的旧哈希，本段说明在运行后加入，不改变原运行记录
