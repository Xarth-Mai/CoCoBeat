# 验证策略

## 检查入口

```sh
cargo xtask doctor
cargo xtask check
cargo run --locked -p cocobeat-lab -- timing-sim
cargo run --locked -p cocobeat-lab -- generate-dev
cargo run --locked -p cocobeat-game -- --replay path/to/session.json
cargo run --locked -p cocobeat-game -- --visual-smoke target/cocobeat-preview.png
cargo run --locked -p cocobeat-game -- --startup-smoke target/cocobeat-startup.png
```

`doctor` 检查 Rust/Cargo/rustfmt/Clippy 与依赖边界；`check` 执行依赖边界、格式、Clippy 和整个 workspace 的测试，失败返回非零状态

`timing-sim` 输出纯软件实验；`generate-dev` 输出原创 PCM16 WAV 和校验清单；`--replay` 拒绝身份或格式不匹配，并使用同一个 core 重建规则结果；`--visual-smoke` 只保存预设 3D 场景 PNG，音频、输入和玩法均不参与，不能作为可玩性验收

## 当前证据

品牌模块首次交付为 `4654512`，主线程接线提交为 `356d675`；最新隔离验证覆盖品牌追加提交 `56faef2`，包括 7.20 秒完成的停靠回弹与 Ready 眼睛循环；以下保留各阶段证据，后续未提交改动不自动继承已有验证结论

2026-10-02 本地切片基线的 `cargo xtask check` 退出码为 0，完整 workspace 的 40 项测试、依赖边界、格式与 Clippy 均通过，日志位于 `target/first-slice-check.log`；本轮测试包含 Kira 初始 Playing 状态不能当作回调确认、游标倒退拒绝、停滞到期与探针统计修订

| 范围 | 结果 | 证据边界 |
|---|---|---|
| schema 4 项 + core 9 项 | PASS | 整数时间、溢出、独立预期、判定边界、水位/epoch、去重、720 种交付排列与有界 Resonance |
| ClockBridge 7 项 | PASS | 单调映射、历史、外推边界、暂停/重启/设备丢失状态；无硬件参与 |
| Replay 5 项 | PASS | 格式/大小限制、损坏拒绝、保存失败保留原文件，60/144 Hz 与 500 ms 卡顿批次重放一致 |
| runtime 其余 9 项 + lab 3 项 + xtask 3 项 | PASS | Kira 原生 MockBackend 的首次回调确认、输入边沿、Session 捕获与重放、停滞/倒退、纯音效与探针统计、呈现状态、内容字节/标注、CLI 参数及模块边界；无听感验收 |
| 软件计时矩阵 | PASS | `target/debug/cocobeat-lab timing-sim target/timing-sim`：30/64/300/600 秒 × 4 情景，共 198,816 个采样，0 个超出声明的不确定性；含采样误差与卡顿时最大误差为 33 帧，错误地使用消费时刻会达到 5,761 帧 |
| 原创内容与来源 | PASS | `target/debug/cocobeat-lab generate-dev target/dev-assets` 生成 3,072,000 帧 PCM16 双声道 WAV，SHA-256 与资源台账一致，5 项台账哈希全部匹配 |
| 可执行文件与 Replay CLI | PASS | `cargo build --locked -j 1 -p cocobeat-game -p cocobeat-lab`；合成历史经 `--replay` 得到 18 facts / 22 events，内容错配、规则错配和截断均非零退出，记录见 `target/replay-smoke/results.txt` |
| 呈现截图 | PASS | `target/debug/cocobeat-game --visual-smoke target/visual-smoke.png` 在 AMD RX 6650 XT / RADV Vulkan 离屏渲染并退出 0，已检查双角色、局部/共同环、街道和 HUD；未参与音频、窗口交互、品牌动画或可玩性验收 |
| 真实音频、输入、平台与真人体验 | NOT RUN | 需要下表列出的设备和参与者证据 |

图形检查使用 Bevy 原生离屏目标、显式 UI 相机、同步管线编译和固定时间步，避免 Xvfb/Vulkan 呈现限制；当前离屏日志有未指定 ShadowLodOrigin 的警告，截图不能作为灯光阴影或性能验收

CI 只检查单 Linux 上的 schema/core/replay/xtask，以及全仓库格式和依赖图；本地 `cargo xtask check` 覆盖完整 workspace，Windows/Linux 四目标发行构建使用手动 Action，见 [构建与 CI](build-release.md)

## 品牌集成证据

2026-10-02，`CARGO_BUILD_JOBS=1 cargo xtask check` 退出码为 0，52 项测试、格式、Clippy 与依赖边界全部通过，日志为 `target/brand-integration-check.log`；新增范围含品牌时间线与 PCM、完整输入门控、失败提示层级和 Windows 资源编译依赖边界

`cargo build --locked -j 1 -p cocobeat-game -p cocobeat-lab` 通过，日志为 `target/brand-integration-build.log`；随后 `target/debug/cocobeat-game --startup-smoke target/startup-integrated.png` 在 AMD RX 6650 XT / RADV Vulkan 离屏运行到 Complete 并退出 0，已检查同一 Logo 停靠、Ready 场景和 HUD 的组合画面

启动截图只运行品牌和场景呈现，不创建音频管理器或消费物理输入；它不证明正常窗口的音画同步、完整设备生命周期或实际按钮操作，相关边界由软件单测和下列设备验收分别覆盖

Windows 图标资源通过 `llvm-rc` 和 `llvm-cvtres` 编译为 x86-64/ARM64 资源对象，包含 6 个尺寸和 1 个图标组；Linux `.desktop` 通过 `desktop-file-validate`，两平台打包配置和 shell 语法已检查，Linux 以占位二进制验证 tar 布局与执行权限；最终 Windows EXE、真实游戏发行包、桌面图标显示与远端四目标构建仍为 NOT RUN

516 条第三方依赖与 Cargo metadata/lock 一致，23 条资源来源哈希匹配；这项检查证明台账与文件一致，不推断原始品牌参考图的再分发许可

## 里程碑提交检查

2026-10-02，确定性规则与 Replay 里程碑 `378f95b` 的独立快照通过 21 项测试、Clippy、格式与依赖边界，日志为 `target/milestone-core-check.log`

本地双人运行时与品牌接线候选基于品牌提交 `d0d3cfd`，已通过独立快照的完整 `cargo xtask check`，55 项测试、Clippy、格式与依赖边界全部通过，日志为 `target/milestone-runtime-check.log`；快照与同时进行的品牌动画修改分开验证

提交候选的 `cargo build --locked -j 1 -p cocobeat-game -p cocobeat-lab` 退出 0，日志为 `target/milestone-runtime-build.log`；从 `/tmp` 运行生成的游戏 `--startup-smoke` 同样退出 0，截图 `target/milestone-runtime-startup.png` 已检查同一 Logo 停靠、Ready 场景与 HUD，日志为 `target/milestone-runtime-startup.log`；该检查也确认嵌入资源无需仓库工作目录，仍未播放音频或消费物理输入

本次还用 Kira MockBackend 复现并修复了异步暂停竞态：同一回调前恢复再暂停会错误停留在 Playing，记录见 `target/audio-transition-repro.log`；修复后合并同帧最终意图，并等待相反命令确认，回归覆盖双向转换、未确认状态、停止优先与新句柄重置，窄测日志为 `target/audio-transition-check.log`；它们不代表真实设备音画同步已验收

## Ready 菜单循环接线

2026-10-02，以品牌提交 `5e4839a` 创建独立快照 `/tmp/cocobeat-milestone-menu`，完整 `cargo xtask check` 退出 0，59 项测试、Clippy、格式及依赖边界通过，日志为 `target/milestone-menu-check.log`；`cargo build --offline --locked -j 1 -p cocobeat-game` 通过，日志为 `target/milestone-menu-build.log`

生产 `suspend_intro` 在品牌 Advance 前按 `Ready && menu_open` 设置 `idle_enabled`；新增 ECS 测试经真实 Bevy 焦点消息捕获验证 Ready、其他游戏阶段、菜单可见性与失焦控制，不会解锁启动输入或自行启动歌曲；品牌单测覆盖 6.60 秒 Complete、焦点冻结/恢复首帧、禁用归零与 24 秒循环接缝

Main menu 保存 Replay 后停止歌曲，重建同 epoch 空会话，下一次明确 Start 才推进 epoch；键盘/手柄回归覆盖返回菜单、清空待处理操作、保留绑定与设备归属、确认键释放屏障，ECS 测试验证返回 Ready 后重新开启循环；保存失败及真实设备操作不属于这个无音频 ECS 测试

从 `/tmp` 执行快照构建的游戏 `--startup-smoke` 退出 0，实际运行品牌时间线和生产菜单控制系统，在 Complete 后的 Ready 循环 6.1 秒截取 `target/milestone-menu-startup.png`，已检查蓝眼侧看、同一 Logo 停靠、场景与 HUD；GPU 为 AMD RX 6650 XT / RADV Vulkan，日志为 `target/milestone-menu-startup.log`，源码与产物身份记录在 `target/milestone-menu-evidence.json`

本轮复用原有 Kira 音频生命周期和输入解锁路径，未引入新的音频管理器或固定解锁时长；生产接线的软件检查与 GPU 呈现通过，真实窗口音画同步、听感、物理输入和品牌线程后续微调仍需分别验收

品牌随后提交 `56faef2`：6.60 秒到位并显露界面，7.20 秒完成位移/剪切回弹后进入 Complete；主线程沿用 `is_complete()`，6.60–7.20 秒仍屏蔽菜单与歌曲操作，三次落点音效时刻和 Ready 开关均不变

对 `56faef2` 的独立快照 `/tmp/cocobeat-menu-followthrough` 补跑完整 check，59 项测试、Clippy、格式及依赖边界再次通过，游戏构建退出 0；从 `/tmp` 运行 `--startup-smoke` 退出 0，已检查最新 Ready 循环截图 `target/menu-followthrough-startup.png`；日志分别为 `target/menu-followthrough-check.log`、`target/menu-followthrough-build.log` 和 `target/menu-followthrough-startup.log`，提交与产物哈希见 `target/menu-followthrough-evidence.json`，本次仍无真实音频或物理输入参与

## 尚待验收

- 硬件：Kira 定时点击、loopback 输出偏移、输入延迟、漂移和设备切换；软件游标的实验误差配置不代替测量
- 平台：Windows/Linux 各 x86-64/ARM64 完整构建及干净机器运行，GPU/音频后端和真实键盘/手柄分别验证
- 输入：双手柄、混合输入、菜单、重绑定、USB/蓝牙、失焦及断连/重连，见 [验收矩阵](platform-input.md)
- 体验：听感、伙伴感知、沉默、模仿、连点、共享确认延迟和 Anchor 预告，保存具体行为、对照与访谈

之后再增加 canonical 编码回读、SongPackage 事务/哈希、MIR 标注、QUIC 模拟和两台真实机器测试，当前开发 PCM 与 JSON Replay 不代表这些能力已实现

每项功能记录责任、真值来源、失败行为、实际检查与可重放证据；测试计数与模拟分数只证明相应软件范围
