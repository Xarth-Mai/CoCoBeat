# Replay 工作台受控验证

复制当前生产工作台和 Replay 诊断源码，在副本中附加合成 `KeyboardInput` 驱动与 Bevy 原生 `Screenshot`，生产输入、布局和重放逻辑保留原样；每次使用新的证据目录

```sh
python3 tools/replay-workbench-check/check.py prepare NEW_EVIDENCE_DIR
python3 tools/replay-workbench-check/check.py build NEW_EVIDENCE_DIR CARGO_BUILD_JSONL BUILD_SOURCE_MANIFEST_JSON
python3 tools/replay-workbench-check/check.py run NEW_EVIDENCE_DIR PACKAGE REPLAY EXPECTED_REPORT_JSONL
```

`CARGO_BUILD_JSONL` 来自当前源码的成功 `cargo build --offline --locked -j1 -p cocobeat-lab --message-format=json`，构建脚本核对配套源码 SHA-256 清单后，仅调用 `rustc` 并使用 JSONL 中明确记录的 extern；`EXPECTED_REPORT_JSONL` 应由需验证的同一包与 Replay 预先生成，当前固定案例为 76 条记录、首水位 -1632、全局索引 71 的 Free Sync；旧版报告用于逐项检验字段与顺序，JSONL 原始字节另用正式 CLI 比较

运行需要已有 gamescope 和 Vulkan 环境，每个进程使用独立 headless 显示与工作目录，按 1280×800 和 640×480 逐条检查事实与事件索引、整数定位、Free Sync 双 Hit 标记、负水位原值、虚拟列表和详情滚动，并保存五张原生 PNG；原包、Replay 和期望报告的完整字节哈希必须保持一致

`prepared.json`、`instrumentation.patch`、`build.json`、`run.json` 与进程日志记录源码、注入边界、extern、命令、退出码、状态断言及图像哈希，截图仍需人工视觉检查；这是软件注入与 GPU 渲染证据，物理输入、扬声器和真人验收另行完成
