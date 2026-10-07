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
cargo run --locked -p cocobeat-game -- --settings-smoke target/cocobeat-settings.png
cargo run --locked -p cocobeat-game -- --locale-smoke zh-HK target/cocobeat-locale.png
cargo run --locked -p cocobeat-game -- --menu-smoke en-GB target/cocobeat-menu.png
cargo run --locked -p cocobeat-game -- --language-smoke uk target/cocobeat-languages.png
cargo run --locked -p cocobeat-game -- --quality-smoke low target/cocobeat-quality.png
cargo run --locked -p cocobeat-game -- --settings-page-smoke graphics zh-CN target/cocobeat-graphics.png
cargo run --locked -p cocobeat-game -- --settings-page-smoke pacing en-GB target/cocobeat-pacing.png
```

`doctor` 检查 Rust/Cargo/rustfmt/Clippy 与依赖边界；`check` 执行依赖边界、格式、Clippy 和整个 workspace 的测试，失败返回非零状态

`timing-sim` 输出纯软件实验；`generate-dev` 输出原创 PCM16 WAV 和校验清单；`--replay` 拒绝身份或格式不匹配，并使用同一个 core 重建规则结果；`--visual-smoke` 保存预设 3D 场景 PNG，`--startup-smoke` 运行生产品牌与 Ready 呈现，设置及语言预览的范围见下文；这些 smoke 均不创建音频输出、读写用户配置或消费输入驱动玩法，但默认 Gilrs 手柄后端仍会初始化并轮询，不能作为物理输入或可玩性验收

## 当前证据

品牌模块首次交付为 `4654512`，主线程接线提交为 `356d675`；最新生产软件补验覆盖 `2329c19` 中的品牌 `67962a3`，CachyOS release 验证覆盖 `7c37972`，Ubuntu 24.04 容器发行基线覆盖 `5949c13`；以下保留各阶段证据，后续未提交改动不自动继承已有验证结论

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

图形检查使用 Bevy 原生离屏目标、显式 UI 相机、同步管线编译和固定时间步，避免 Xvfb/Vulkan 呈现限制；本地切片基线的离屏日志有未指定 ShadowLodOrigin 的警告，截图不能作为灯光阴影或性能验收

CI 只检查单 Linux 上的 schema/core/replay/media/xtask，以及全仓库格式和依赖图；本地 `cargo xtask check` 覆盖完整 workspace，Windows/Linux 四目标发行构建使用手动 Action，见 [构建与 CI](build-release.md)

## 品牌集成证据

2026-10-02，`CARGO_BUILD_JOBS=1 cargo xtask check` 退出码为 0，52 项测试、格式、Clippy 与依赖边界全部通过，日志为 `target/brand-integration-check.log`；新增范围含品牌时间线与 PCM、完整输入门控、失败提示层级和 Windows 资源编译依赖边界

`cargo build --locked -j 1 -p cocobeat-game -p cocobeat-lab` 通过，日志为 `target/brand-integration-build.log`；随后 `target/debug/cocobeat-game --startup-smoke target/startup-integrated.png` 在 AMD RX 6650 XT / RADV Vulkan 离屏运行到 Complete 并退出 0，已检查同一 Logo 停靠、Ready 场景和 HUD 的组合画面

启动截图只运行品牌和场景呈现，不创建音频管理器或消费输入驱动玩法；它不证明正常窗口的音画同步、完整设备生命周期或实际按钮操作，相关边界由软件单测和下列设备验收分别覆盖

Windows 图标资源通过 `llvm-rc` 和 `llvm-cvtres` 编译为 x86-64/ARM64 资源对象，包含 6 个尺寸和 1 个图标组；Linux `.desktop` 通过 `desktop-file-validate`，两平台打包配置和 shell 语法已检查，当时以占位二进制验证 tar 布局与执行权限，真实 Linux 包的后续检查见下文；最终 Windows EXE、桌面图标显示与远端四目标构建仍为 NOT RUN

516 条第三方依赖与 Cargo metadata/lock 一致，23 条资源来源哈希匹配；这项检查证明台账与文件一致，不推断原始品牌参考图的再分发许可

## 里程碑提交检查

2026-10-02，确定性规则与 Replay 里程碑 `378f95b` 的独立快照通过 21 项测试、Clippy、格式与依赖边界，日志为 `target/milestone-core-check.log`

本地双人运行时与品牌接线候选基于品牌提交 `d0d3cfd`，已通过独立快照的完整 `cargo xtask check`，55 项测试、Clippy、格式与依赖边界全部通过，日志为 `target/milestone-runtime-check.log`；快照与同时进行的品牌动画修改分开验证

提交候选的 `cargo build --locked -j 1 -p cocobeat-game -p cocobeat-lab` 退出 0，日志为 `target/milestone-runtime-build.log`；从 `/tmp` 运行生成的游戏 `--startup-smoke` 同样退出 0，截图 `target/milestone-runtime-startup.png` 已检查同一 Logo 停靠、Ready 场景与 HUD，日志为 `target/milestone-runtime-startup.log`；该检查也确认嵌入资源无需仓库工作目录，仍未播放音频或验收物理输入玩法

本次还用 Kira MockBackend 复现并修复了异步暂停竞态：同一回调前恢复再暂停会错误停留在 Playing，记录见 `target/audio-transition-repro.log`；修复后合并同帧最终意图，并等待相反命令确认，回归覆盖双向转换、未确认状态、停止优先与新句柄重置，窄测日志为 `target/audio-transition-check.log`；它们不代表真实设备音画同步已验收

## Ready 菜单循环接线

2026-10-02，以品牌提交 `5e4839a` 创建独立快照 `/tmp/cocobeat-milestone-menu`，完整 `cargo xtask check` 退出 0，59 项测试、Clippy、格式及依赖边界通过，日志为 `target/milestone-menu-check.log`；`cargo build --offline --locked -j 1 -p cocobeat-game` 通过，日志为 `target/milestone-menu-build.log`

生产 `suspend_intro` 在品牌 Advance 前按 `Ready && menu_open` 设置 `idle_enabled`；新增 ECS 测试经真实 Bevy 焦点消息捕获验证 Ready、其他游戏阶段、菜单可见性与失焦控制，不会解锁启动输入或自行启动歌曲；品牌单测覆盖 6.60 秒 Complete、焦点冻结/恢复首帧、禁用归零与 24 秒循环接缝

Main menu 保存 Replay 后停止歌曲，重建同 epoch 空会话，下一次明确 Start 才推进 epoch；键盘/手柄回归覆盖返回菜单、清空待处理操作、保留绑定与设备归属、确认键释放屏障，ECS 测试验证返回 Ready 后重新开启循环；保存失败及真实设备操作不属于这个无音频 ECS 测试

从 `/tmp` 执行快照构建的游戏 `--startup-smoke` 退出 0，实际运行品牌时间线和生产菜单控制系统，在 Complete 后的 Ready 循环 6.1 秒截取 `target/milestone-menu-startup.png`，已检查蓝眼侧看、同一 Logo 停靠、场景与 HUD；GPU 为 AMD RX 6650 XT / RADV Vulkan，日志为 `target/milestone-menu-startup.log`，源码与产物身份记录在 `target/milestone-menu-evidence.json`

本轮复用原有 Kira 音频生命周期和输入解锁路径，未引入新的音频管理器或固定解锁时长；生产接线的软件检查与 GPU 呈现通过，真实窗口音画同步、听感、物理输入和品牌线程后续微调仍需分别验收

品牌随后提交 `56faef2`：6.60 秒到位并显露界面，7.20 秒完成位移/剪切回弹后进入 Complete；主线程沿用 `is_complete()`，6.60–7.20 秒仍屏蔽菜单与歌曲操作，三次落点音效时刻和 Ready 开关均不变

对 `56faef2` 的独立快照 `/tmp/cocobeat-menu-followthrough` 补跑完整 check，59 项测试、Clippy、格式及依赖边界再次通过，游戏构建退出 0；从 `/tmp` 运行 `--startup-smoke` 退出 0，已检查最新 Ready 循环截图 `target/menu-followthrough-startup.png`；日志分别为 `target/menu-followthrough-check.log`、`target/menu-followthrough-build.log` 和 `target/menu-followthrough-startup.log`，提交与产物哈希见 `target/menu-followthrough-evidence.json`，本次仍未验收真实音频或物理输入玩法

## 品牌 67962a3 生产补验

2026-10-02，从固定提交 `2329c191bb28f0de78c9cfd1f65ea1cbfaca7020` 创建归档快照 `/tmp/cocobeat-brand-67962a3`，134 个源码文件内容保持不变，`brand_intro.rs` 与 `brand.wgsl` 逐字节匹配品牌提交 `67962a3`；在该快照执行 `CARGO_BUILD_JOBS=1 cargo xtask check` 和 `cargo build --offline --locked -j1 -p cocobeat-game` 均退出 0，59 项测试、Clippy、格式与依赖边界通过

从 `/tmp` 以 `WGPU_BACKEND=vulkan` 运行生成的游戏 `--startup-smoke`，AMD RX 6650 XT / RADV Vulkan 离屏渲染退出 0，`target/brand-67962a3-startup.png` 已由主线程视觉检查正常；构建、检查和启动日志分别为 `target/brand-67962a3-{build,check,startup}.log`，完整命令、环境、源码与产物哈希见 `target/brand-67962a3-evidence.json`，首次受限环境无法枚举 GPU 的日志另存为 `target/brand-67962a3-startup-sandbox.log`

本次通过的是生产模块、Ready 菜单门控与终态 GPU 呈现；6.15–7.20 秒动态轨迹另见 [品牌第六版连续预览](../assets/brand/VALIDATION.md#渲染证据)，终态截图不能单独证明动作过程；真实音频同步、物理输入玩法和平台图标显示仍为 NOT RUN，本次未重建 release 包

## 设置与基础显示里程碑

2026-10-02，在工作区执行 `CARGO_BUILD_JOBS=1 cargo xtask check` 和 `cargo build --offline --locked -j1 -p cocobeat-game`，67 项测试、Clippy、格式、依赖边界和构建均通过；日志为 `target/settings-final-{check,build}.log`，实际源码 SHA-256、执行命令和产物身份记录在 `target/settings-milestone-evidence.json`

新增检查覆盖配置损坏和版本拒绝、原子替换与失败保留、分辨率列表的草稿与取消、15 秒预览到期/主动回退、确认保存和保存失败，以及键盘/手柄设置路由与确认键释放屏障；显示状态测试要求模式报告与几何尺寸一致，待确认的模式不能提前保存，普通窗口拒绝缩放则保存实际尺寸；独立设置路径 harness 验证绝对 XDG/HOME 路径及无目录时明确报错，Windows 原生路径执行尚未验收

`--settings-smoke` 显式模拟 1280×800 表面和 640×480 全屏渲染尺寸，GPU 绘制实际 3D 目标图像、letterbox 与独立 UI 相机；`--startup-smoke` 和 `--visual-smoke` 同时回归品牌 Ready 循环与场景反馈。三项从 `/tmp` 使用 AMD RX 6650 XT / RADV Vulkan 执行并退出 0，截图 `target/settings-milestone-{settings,startup,visual}.png` 已检查，低分辨率场景外的 Logo、文字和 HUD 保持原生清晰度，设置面板不遮挡玩家标签

这些命令不读写用户设置，不启用音频播放或输入驱动的玩法，也不模拟原生窗口管理器；桌面精确可用区域、真实无边框切换、手动缩放/最大化、DPI、跨屏与 Windows/Linux 物理键盘/手柄设置操作仍为 NOT RUN。Winit 当前接线只有显示器减边框的尺寸上界，不能当作可用桌面工作区的完成证据

共用 target 缓存曾使 `xtask` 指向旧品牌快照，该次运行已停止并排除，诊断日志保留为 `target/settings-stale-snapshot-check.log`；随后只清理 `xtask` 包并确认从当前工作区重新编译、执行。后续共享 target 的快照检查必须确认 xtask 实际源码目录，不能根据退出码推断检查了当前源码

## 国际化与字体里程碑

2026-10-02，在工作区执行 `CARGO_BUILD_JOBS=1 cargo xtask check` 和 `cargo build --offline --locked -j1 -p cocobeat-game`，完整 workspace 的 75 项测试、Clippy、格式、依赖边界和构建均为 PASS；日志为 `target/i18n-final-{check,build}.log`，源码、可执行文件及截图的 SHA-256 与执行命令记录在 `target/i18n-evidence/evidence.json`

首批 13 个语言变体为 `zh-CN`、`en-US`、`en-GB`、`ja`、`ko`、`zh-TW`、`zh-HK`、`es-419`、`pt-BR`、`fr`、`de`、`ru`、`uk`，每份 catalog 有 91 个翻译键，键集和占位符一致；测试覆盖系统语言与地区匹配、英文 US/GB 和繁体 TW/HK 的独立保存、旧 v1 配置保留显示设置、语言与显示草稿共同提交/取消/超时回退，以及保存失败保留旧配置。持久通知保存翻译键和参数，切换语言后重绘；玩家可见错误使用本地化操作提示，诊断细节保留在日志

所有 UI 显式使用 Noto Sans，中文简体/台湾/香港、日文和韩文分别选择对应字体；语言本名使用各自字体，13 组地区旗帜由已记录来源的 SVG 导出 PNG，语言列表分为 5/5/3 行三页。软件检查覆盖字体解析、地区字体与旗帜映射、切换语言后的 HUD、原生名称和分页可见性；来源和许可见 [字体](../assets/fonts/README.md) 与 [旗帜](../assets/flags/README.md)

从 `/tmp` 使用 AMD RX 6650 XT / RADV Vulkan 执行 29 次 GPU 离屏检查，均退出 0，`target/i18n-evidence/` 保存 13 张 `locale-CODE.png` 设置页、13 张 `menu-CODE.png` Ready 菜单及 `language-zh-CN.png`、`language-zh-HK.png`、`language-uk.png` 三页语言列表，尺寸均为 1280×800；主线程与国际化 agents 已逐张检查字形、换行、地区旗帜与页面可读性，实际 Ready 菜单与生产路径共用 `game_status`

`--locale-smoke CODE PNG` 和 `--language-smoke CODE PNG` 显式模拟 1280×800 表面与 640×480 全屏场景，`--menu-smoke CODE PNG` 使用同尺寸表面的 Ready 场景；三者运行品牌时间线和 Ready 循环，使用固定时间步加速离屏预览，不创建音频输出、不读写用户配置，不消费输入驱动玩法。原生 WM、DPI、跨屏、Windows/ARM64、物理键盘/手柄操作、真实音画同步和母语真人校对仍为 NOT RUN

当前 `Cargo.lock` 使用 Bevy 0.19.1 / Parley 0.9.0，离屏日志保留 `ICU4X data error: No segmentation model for complex script: Chinese/Japanese` 诊断；已检查截图的 CJK 字形与换行通过，词边界分词数据缺口仍存在。上游 [Bevy #25674](https://github.com/bevyengine/bevy/pull/25674) 已合入可选 `complex_script_segmentation`，用于提供复杂文字词边界数据，2026-10-02 的[最新稳定版仍为 0.19.1](https://docs.rs/crate/bevy/latest)，尚未包含该特性；等待稳定版发布后启用并重跑分词与界面检查，本轮保留诊断日志、不引入预发布依赖

## 画质与帧率设置里程碑

2026-10-02，在工作区执行 `CARGO_BUILD_JOBS=1 cargo xtask check` 和 `cargo build --offline --locked -j1 -p cocobeat-game`，完整 workspace 的 81 项测试、Clippy、格式、依赖边界和构建均为 PASS；日志为 `target/quality-final-{check,build}.log`，115 个源码文件与可执行文件的 SHA-256、执行命令及退出码记录在 `target/quality-evidence/evidence.json`

低、中、高预设与 Custom 状态、MSAA 关闭/2×/4×、雨量关闭/25%/50%/100%、雾、阴影及 Bloom 均已接入，默认中档；软件检查覆盖预设矩阵、独立项改动后的 Custom 标记、场景组件生效与关闭装饰效果后保留核心反馈，画质设置应用于 3D 场景，UI 相机保持独立。13 份 catalog 从国际化里程碑的 91 个键扩展到 113 个键，键集与参数合约检查通过；新增依赖仅为 Bevy 原生 `bevy_post_process` feature 带入的 0.19.1 包，当前台账记录 517 个第三方包

画质、帧率、语言与显示共用设置草稿，保存成功后才提交，显示预览期间保持原画质、语言与帧率；测试覆盖子页操作、应用、取消、恢复默认、15 秒预览超时和保存失败，子页返回保留草稿，关闭设置不恢复歌曲。旧 v1 配置自动补入默认画质与帧率，合法独立项与预设标签不一致时保留值并标记 Custom；低于 1000 mHz 的有限帧率值被拒绝，失败保存保留旧文件

帧率值以 mHz 保存，默认在首次显示观察后选用当前显示器报告的最高刷新率；选项为每 60 一档、追加最高档并去重、无限制，未知刷新率时使用 60，VSync 独立控制且默认关闭。模拟观察覆盖 48 / 59.94 / 60 / 120 / 144 / 280 Hz、跨屏后有限值超上限时收敛及无限制保留，活动值、草稿和预览回退值同步规范化；这些检查不涉及真实跨屏事件或原生呈现结果

生产 CPU 帧率门控的墙钟测试在 120 FPS 上限下记录 10 个更新起点，全部相邻间隔达到至少 8,333,334 ns，并检查无限制状态和独立 VSync 命令；PASS 只证明软件门控与 Bevy Window 属性接线，不证明实际 GPU 呈现帧率或显示器同步效果。新增 `Game → Session → Replay` 测试在 60 / 144 / 48 Hz 消费节奏和 500 ms 消费批次下使用同一组捕获历史，映射后的 Hit、规则事件与重放结果一致；500 ms 用例中的捕获时刻仍须落在有效历史窗口内，不涵盖任意时长卡顿或物理输入延迟

`--quality-smoke low|medium|high|off PNG` 渲染固定场景，`off` 关闭 MSAA、雨、雾、阴影与 Bloom；`--settings-page-smoke graphics|pacing CODE PNG` 经生产设置操作进入画质或帧率子页，显式模拟 1280×800 表面与 640×480 场景，使用固定时间步运行品牌到 Ready。两种预览均不创建音频输出、不读写用户配置，不运行生产帧率门控，不能用截图耗时推断实际帧率

本轮 46 次 GPU 离屏检查全部退出 0，包含低/中/高/off 共 4 张画质场景、13 个语言变体各自的设置父页/画质页/帧率页共 39 张，以及简中/香港繁中/乌克兰语共 3 张语言列表；主线程与国际化 agents 已逐张检查，结果为 PASS。截图均为 1280×800，640×480 场景保留 letterbox，UI 使用原生分辨率；设置父页 8 行、画质页 7 行、帧率页 3 行均完整可读且无裁切，关闭装饰效果后核心反馈仍可辨识。115 个源码文件、可执行文件、46 张 PNG 和日志的哈希全部匹配 `target/quality-evidence/evidence.json`；日志含 904 条已知 ICU CJK 词边界诊断，未发现其他 WARN/ERROR/error，这些证据不代替真实设备验收

ICU CJK 词边界诊断沿用上文记录的稳定版限制，继续保留日志；真实窗口、WM、DPI、跨屏、呈现 FPS、VSync、Windows/ARM64、物理输入延迟与操作均为 NOT RUN，母语真人校对和音画同步仍需分别验收

## 原生 Linux release 证据

2026-10-02，提交 `7c37972` 的归档快照在 CachyOS x86_64 完成 release 构建和 workflow 打包，均退出 0；源码身份、构建命令、二进制与 tar.gz 的 SHA-256 见 [构建记录](build-release.md#本机-linux-release-验证)，证据目录为 `target/linux-release-evidence/7c3797261960/`

从独立解包目录 `/tmp/cocobeat-linux-release-unpacked-7c3797261960` 执行 `--help` 退出 0；对证据目录内既有合成 Replay 执行 `--replay FILE`，合法样例退出 0 并得到 18 facts / 22 rule events / epoch 64，内容身份错配、规则身份错配和截断样例均退出 1，命令与输出见 `cli-validation.json`

在同一解包目录执行以下命令，AMD RX 6650 XT / RADV Vulkan 实际离屏渲染并退出 0，1280×800 PNG 已视觉检查正常；日志为 `startup.log`，截图为 `startup.png`，图片哈希、命令及 cwd 记录在 `acceptance.json`

```sh
WGPU_BACKEND=vulkan ./bin/cocobeat-game --startup-smoke /home/lzzz/MyProjects/CoCoBeat/target/linux-release-evidence/7c3797261960/startup.png
```

本轮 release、真实打包、CLI 和 GPU 离屏启动均为 PASS；产物要求 `GLIBC_2.44`，不证明 Ubuntu 24.04 兼容，Windows/ARM64 构建、干净机器运行、本发行包的音频与物理输入、真人体验仍为 NOT RUN，四目标发行门槛未完成

## Ubuntu 24.04 容器发行基线

2026-10-02，实际源码 `5949c13ce75e08d880648d72205259097e5e6ff0` 在官方 `docker.io/library/ubuntu:24.04` 的 Linux amd64 镜像中验证，manifest digest 为 `sha256:f610ab94648195aa356059f5b41d6085c9d4d903c072430cdd1af7bdb646106b`，工具链为 Rust/Cargo 1.98.1；记录位于 `target/ubuntu-build-evidence/e9345f9/evidence.json` 及该目录的 `artifacts/package-acceptance.json`，目录名沿用准备阶段

`CARGO_BUILD_JOBS=1 CARGO_NET_OFFLINE=true cargo xtask check` 退出 0，92 项测试、workspace 格式与 Clippy、依赖边界均为 PASS；`cargo build --offline --locked --release -j1 -p cocobeat-game` 退出 0，使用正式 fat LTO 发行参数；既有 Linux workflow 布局生成的 62 文件 tar.gz 通过架构、权限、文件哈希及独立解包检查，产物身份见 [构建记录](build-release.md#ubuntu-2404-容器发行基线)

容器内 `readelf --version-info /target/release/cocobeat-game` 确认最高要求 `GLIBC_2.39`，`ldd /target/release/cocobeat-game` 全部解析；从 `/artifacts` 执行 `/artifacts/unpacked/bin/cocobeat-game --help` 和 `/artifacts/unpacked/bin/cocobeat-game --replay /artifacts/synthetic-64s-valid.replay.json` 均退出 0，合成 Replay 输出 `Replay OK: 18 facts, 22 rule events, epoch 64`

本轮 PASS 仅覆盖本地 Ubuntu 24.04 x86-64 的构建、打包及 CLI 基线；Ubuntu 图形/窗口、音频、物理输入、真人体验、干净桌面安装、Windows/ARM64 和 GitHub workflow 执行均为 NOT RUN，合成 Replay 不构成硬件验收

## 尚待验收

- 硬件：Kira 定时点击、loopback 输出偏移、输入延迟、漂移和设备切换；软件游标的实验误差配置不代替测量
- 平台：Windows x86-64/ARM64、Linux ARM64 完整构建，各目标远端发行构建及干净机器运行；Linux x86-64 已有上述本地容器基线，GPU/音频后端和真实键盘/手柄仍需分别验证
- 输入：双手柄、混合输入、菜单、重绑定、USB/蓝牙、失焦及断连/重连，见 [验收矩阵](platform-input.md)
- 体验：听感、伙伴感知、沉默、模仿、连点、共享确认延迟和 Anchor 预告，保存具体行为、对照与访谈

之后再增加 canonical 编码回读、SongPackage 事务/哈希、MIR 标注、QUIC 模拟和两台真实机器测试，当前开发 PCM 与 JSON Replay 不代表这些能力已实现

每项功能记录责任、真值来源、失败行为、实际检查与可重放证据；测试计数与模拟分数只证明相应软件范围


## 小窗口设置与桌面工作区

2026-10-02，基于 `0d3fc8b` 完成结构化设置行与原生滚动，父页、画质、帧率、分辨率、全部 13 个语言选项及说明/错误都可聚焦；当前行自动进入可见区域，超高行先用上下键逐段阅读，确认与返回保留原语义；进入显示预览聚焦等待/确认状态，超时回退重置滚动

`CARGO_BUILD_JOBS=1 cargo xtask check` 与 `cargo build --offline --locked -j1 -p cocobeat-game` 通过，完整 workspace 共 88 项测试，格式、Clippy、依赖边界与构建均为 PASS；日志为 `target/display-final-{check,build}.log`，117 个源码与资源文件、可执行文件、执行命令和渲染目标实际几何记录在 `target/display-evidence/evidence.json`。布局窄测运行真实 Bevy/Taffy 和 ScrollPosition，覆盖不同测量行高、180×120/400×300 视口、缩放因子 2、跨页焦点与关闭清理；真实 Noto 换行另由 GPU 截图检查

Windows 通过 winsafe 安全读取显示器 `rcWork`，X11 验证 EWMH 属性并使用桌面工作区与当前显示器的保守交集；Wayland 工作区未知时继续明确显示未知，显示器尺寸仅作上界。显示器、工作区或 DPI 变化后，普通窗口尝试一次尺寸/位置容纳，保留实际回读；全屏与受控窗口推迟调整，WM 拒绝后不持续重试。13 份 catalog 各扩展为 115 键，许可台账共 520 个第三方包

Windows 检查在 Linux 上以独立官方 Rust 1.98.1 执行 `cargo check --offline --locked -j1 -p cocobeat-runtime --target x86_64-pc-windows-gnu`，使用 `target/windows-cross-toolchain/` 的私有编译器和标准库、独立 `target/windows-cross-check/` 缓存；最终退出 0、零 warning，114 个输入文件起止哈希一致，见 `target/windows-cross-toolchain/runtime-final-check.json`。最初混用 Arch 编译器和官方标准库产生 E0514，原日志保留；系统工具链未修改。该证据仅覆盖 Windows 条件代码编译，Windows 链接、运行、图标和真实工作区行为均为 NOT RUN

`--viewport-smoke main|graphics|pacing|languages|ready CODE WIDTH HEIGHT SCALE ROW PNG` 使用实际 ImageRenderTarget 缩放与生产菜单行，ROW 为从 0 开始的焦点行号，ready 只接受 0；命令不播放音频、不读写用户设置，不启动生产帧率门控。`VIEWPORT_GEOMETRY` 记录物理/逻辑视口及面板/当前行实测边界，设置行不可达会使检查失败，超高行记录当前可见部分；Ready 模式只记录边界以暴露既有问题。截图使用与相机相同的 handle 和 scale，初轮 scale 不匹配产生的两张黑图保存在 `diagnostic-scale-target/` 并排除通过结果

本轮使用 AMD RX 6650 XT / RADV Mesa 26.2.3 Vulkan 完成 27 次离屏渲染，均退出 0：25 张设置截图逐张视觉检查 PASS，覆盖 13 个语言变体、连续语言列表、180×120/320×240/400×300/640×360 及 1280×800 scale 1/2；可容纳的当前行完整可见，180×120 德语超高说明行验证首段可见，双向分页由真实布局窄测覆盖。未选中行可位于滚动区域外；字体、英美/英国及台湾/香港地区旗帜均符合所选项

两张 Ready 截图为视觉 FAIL：180×120 香港繁体的顶部菜单项越界，320×240 德语当前选项与 Logo/辅助 HUD 重叠，属于已有整块状态文本的小窗口缺口；本次未将这些页面算作通过，下一步复用结构化行使当前菜单项和必要提示可达。截图与日志 SHA 均匹配证据清单；日志含 450 条已知 ICU CJK 词边界诊断，无其他 WARN/ERROR/error

真实 Windows/Linux WM、跨屏、原生 DPI、键盘/手柄操作与母语真人校对仍为 NOT RUN；离屏缩放与合成平台观测不代替这些证据


## 小窗口游戏菜单

2026-10-02，基于 `80a740e` 修复上节两处 Ready 菜单视觉 FAIL：游戏菜单与设置共用结构化行和原生滚动，12 个操作保留既有次序，控制说明、双人绑定、输入状态、错误和计时信息成为可聚焦的只读行；只读行确认不触发操作，超高行先上下分页。绑定期间继续显示必要提示和错误；菜单页切换、失焦、断连及歌曲状态变化重置滚动，捕获时间戳、同批次设置门控和按住键释放屏障保持原有行为。删除 13 份 catalog 中不再使用的 `menu.selection`，每份现有 114 键，依赖未变

`CARGO_BUILD_JOBS=1 cargo xtask check` 与 `cargo build --offline --locked -j1 -p cocobeat-game` 均退出 0，完整 workspace 的 91 项测试、Clippy、格式、依赖边界与构建为 PASS，日志为 `target/menu-final-{check,build}.log`；新增行为检查覆盖信息行确认、超高行导航、同批次菜单激活、绑定提示保留和四种游戏阶段的呈现，真实 Bevy/Taffy 布局检查补验相同行号、行数及几何下的页面切换重聚焦

`--viewport-smoke main|graphics|pacing|languages|ready|paused|finished|fault CODE WIDTH HEIGHT SCALE ROW PNG` 现在对游戏菜单和设置页均检查选中行实测几何，ROW 为从 0 开始的有效行号，超高行要求当前片段可见。Ready 等待生产品牌完成及菜单循环 6.1 秒；Paused、Finished、Fault 使用相应阶段和位置，等待品牌完成后稳定 3 帧。预览复用生产菜单呈现及导航函数，仍不播放音频、不读写用户配置、不启动生产帧率门控或消费物理输入驱动玩法

本轮在 Linux 的 AMD RX 6650 XT / RADV Vulkan 上从 `/tmp` 完成 28 次离屏渲染，全部退出 0，主线程与三个 agents 已逐张目检 PASS：13 个语言变体的 640×360 Ready 菜单、11 张小窗口/游戏阶段/DPI/普通尺寸菜单，以及 4 张设置/语言/画质/帧率回归图。包括原有失败的 180×120 香港繁体与 320×240 德语、180×120 超高德语控制说明、暂停/结束/故障信息和 1280×800 scale 2。选中行或超高行首段可见，Logo 与菜单分开，非选中行的滚动裁剪符合预期；超高行双向分页由布局及输入行为测试覆盖

117 个源码与资源文件、可执行文件、28 张 PNG 和日志 SHA-256 均与 `target/menu-evidence/evidence.json` 匹配；GPU 日志含 692 条已知 ICU CJK 词边界诊断，无其他 WARN/ERROR/error。英式英语图初次批量目检被误判暗底，随后独立读取同一路径并核对原哈希确认画面正常，未重捕或替换文件，最终为 PASS

Windows 目标检查仍在 Linux 上使用 `target/windows-cross-toolchain/` 内的隔离官方 Rust，执行 `cargo check --offline --locked -j1 -p cocobeat-runtime --target x86_64-pc-windows-gnu --config target/windows-cross-toolchain/cargo-config.toml`，通过 RUSTC 指定私有编译器、CARGO_TARGET_DIR 指定独立缓存，退出 0、零 warning/error，114 个输入文件起止哈希一致；命令、环境和日志见 `target/windows-cross-toolchain/menu-check.json`。没有 Windows 主机参与，Windows 链接与运行、真实 WM/DPI/跨屏、音频与物理输入、母语真人校对均为 NOT RUN


## 显示草稿与实际窗口同步

2026-10-02，基于 `e9345f9` 复现并修复“打开设置 → 外部窗口从 1280×800 缩至 900×700 → 仅改语言 → 应用”误进入显示预览并请求恢复旧尺寸的问题。未编辑的显示字段现在随实际回读更新，分辨率行同步显示自定义尺寸；明确选择分辨率、全屏或恢复默认的字段保留用户意图，预览及回滚等待回读期间不覆盖草稿。生产同步位于 DisplaySystems::Sync 之后，设置动作也使用最新回读

回归先在旧实现退出 101，随后修复后同一行为检查通过；扩展断言覆盖无操作时的尺寸呈现、明确再次选择同值、窗口与全屏尺寸独立编辑、恢复默认及预览/回滚等待。旧失败快照、命令和日志，以及最终单测证据保存在 `target/settings-resize-evidence/`；这些检查模拟外部尺寸回读，不宣称实际拖动窗口已验收

`CARGO_BUILD_JOBS=1 cargo xtask check` 和 `cargo build --offline --locked -j1 -p cocobeat-game` 退出 0，92 项 workspace 测试、Clippy、格式、依赖边界及构建均为 PASS，117 个源码与资源文件以及产物身份见 `target/settings-resize-evidence/evidence.json`。同一源码另通过 Linux 上隔离 Rust 的 Windows GNU runtime 编译检查，命令与 114 个输入文件哈希见 `windows-check.json`；Windows 链接与运行未执行。本轮没有渲染样式或资源修改，未重跑 GPU 截图

早先只读宿主条件核查保存为 `target/platform-readiness/evidence.json`：Linux x86_64 有可用 GPU、DP-1 显示器与 ALSA 播放/采集硬件，但当前用户没有图形桌面会话，X11 查询授权失败；可连接的 PipeWire/Pulse 当时只提供 Dummy Output，udev 未枚举到手柄且当前用户不能读取输入节点。该次核查未启动窗口、播放或录音、读取按键、修改配置或音量；实际 WM/DPI/跨屏、物理音频与 loopback、手柄和真人双人验收继续为 NOT RUN，需先具备用户图形会话、物理音频路径、测量连接和测试设备

## 真实音频后端游标观测

2026-10-02 14:03 UTC 复核时，PipeWire 已枚举 HDMI、USB 与数字音频输出，默认 USB 输出为静音、30% 音量；当前用户仍无图形桌面会话，udev 未枚举到手柄，见 `target/platform-readiness/followup-20261002.json`，早先仅有 Dummy Output 的观测不再代表当前状态

源码 `d1290a21d149b8f520b7f16e44b7a0497f4348c9` 以 `cargo build --offline --locked -j1 -p cocobeat-lab` 构建后，执行 `target/debug/cocobeat-lab audio-probe 30 target/audio-probe-evidence/20261002-d1290a2-30s/probe`，退出 0；CPAL 报告 ALSA `default`、48 kHz 双声道，30 秒探针完成 28,425 次观察、2,811 个游标更新间隔，未报告后端错误，软件游标更新间隔 p50/p95/p99 分别为 10.552203/11.607198/11.622668 ms

命令、193 个源码文件哈希、二进制身份、前后输出状态和 CSV 哈希保存在该目录的 `evidence.json`；二进制 SHA-256 为 `014d0a123c96b3c957eaf63b8b3084abe11caad47b6b67bc88a83e6d8f6da6fc`。执行前后系统默认输出仍为静音、30% 音量，未修改设备或音量；本轮不录音，ALSA 默认设备描述不构成物理声路证明，游标统计不代表扬声器输出延迟，loopback、听感、校准与物理输入仍为 NOT RUN


## 十分钟 Replay 容量

2026-10-03，原 65,536 facts 上限在最密 10 ms 双人水位下于 327.68 秒触发，即使没有 Hit 也无法容纳十分钟；仅扩大 facts 仍可能超过原 8 MiB JSON 上限。现改为 160,000 facts / 20 MiB，保留 v1 JSON、原始事实顺序与现有明确失败行为

`cargo test --offline --locked -p cocobeat-replay` 的 6 项测试通过，默认 debug 整组耗时 188.31 秒；十分钟压力用例记录双方各 20 Hit/s，共 24,000 Hit、144,004 facts，最后确认水位得到 12,000 个共同事件，保存、载入与同一 core 重放后的事件和 Resonance 与 live 一致。最大字段宽度与最大身份转义后的完整容量仍可编码/解码，超限拒绝与 limit+1 有界读取继续通过

`cargo test --offline --locked -p cocobeat-runtime session::tests` 的 4 项测试通过，包含 recorder 满或仅余一条时，拒绝操作不改变规则、序号、水位与诊断；两个 crate 的 all-targets Clippy、格式及差异检查通过。命令、源码 SHA-256 和日志哈希见 `target/long-song-review-20261003/implementation-validation.json`

生产运行时仍使用 64 秒开发歌曲，本次不证明十分钟音频播放或歌曲导入 Ready。core 仍扫描已记录历史，文件大小上限也不等于进程内存上限；真实平台性能、设备和未来网络突发输入分别验收


## 有上限的源音频解码

2026-10-03，`CARGO_BUILD_JOBS=1 cargo xtask check` 完整通过 97 项 workspace 测试、格式、Clippy 与依赖边界；`cargo build --offline --locked -j1 -p cocobeat-lab` 通过。media 使用 Symphonia 0.6.1 顺序读取 WAV/PCM、FLAC、MP3、Ogg Vorbis，限制 512 MiB、192 kHz、十分钟及单块大小，公开接口交付原采样率的有限立体声 PCM，单声道原样复制

正式 `decode-audio` CLI 的 22 项检查通过，覆盖 mono/stereo WAV24、FLAC、MP3、Ogg，拒绝 A-law、不存在/截断/缺页/坏 CRC/虚假 Xing 长度，已有输出保持原字节，新增半成品失败后清理。3 项内置测试包含原始静默与大于 1 的有限幅度保留、资源/非有限值拒绝、codec 裁尾和 Ogg 页回归，两个独立编码的原创微型原件及生成说明见 [样本](../testdata/synthetic/media-import/README.md)

MP3 使用不可 seek 的源，避免上游按 bitrate 估算总时长后裁掉合法尾音；单声道接受前中/前左的布局描述差异，双声道布局仍一致。Ogg 在同一文件上复用 Symphonia CRC 做严格页预检，并检查序号、单一串流、granule 和 EOS，防止库容错恢复跳过损坏页、静默缩短歌曲；PTS 和实际帧数在解码时再次核对

原创 `cocobeat-64.wav` 经最终 lab 输出 3,072,000 帧，与 Python wave 按 PCM16/32768 独立转换的 F32LE 逐字节一致，SHA-256 为 `bd88f48a3b7b641e621a716d7b122d8abd326a6a51177eb06a3f2296896d3538`。完整命令、源码、日志和产物身份见 `target/media-import-evidence/20261003/evidence.json`，正式 CLI 矩阵为 `target/media-review-20261003/lab-results-official.json`

本轮完成源解码软件路径；上限检查不是进程内存/CPU 隔离，未验证 Windows 源解码运行或真人听感。游戏仍使用开发歌曲，标准编码、最终音频回读、SongPackage Ready 和实际导入菜单继续按阶段 05 推进

## 固定 48 kHz 源重采样

`CARGO_BUILD_JOBS=1 CARGO_NET_OFFLINE=true cargo xtask check` 与 `cargo build --offline --locked -j1 -p cocobeat-lab` 均退出 0，完整 workspace 的 100 项测试、格式、Clippy、依赖边界及 lab 构建通过，命令、日志和源码身份见 `target/resample-integration-20261003/workspace-validation.json`

2026-10-03，media 的 `resample_source` 复用有上限的源解码，以 OxiMedia 0.2.1 High 输出有限立体声块，每次回调最多 1024 帧；48 kHz 输入逐样本位精确透传，其他采样率使用窗化 sinc，不裁静默、不归一化或裁幅，完整输出严格核对整数 `ceil(源帧数 × 48000 / 源采样率)`。库类型仅在适配器内，采样率常量复用 schema

3 项新增行为测试通过，覆盖分块、短音频、首尾、1/7/44100/96000/192000 Hz、48 kHz 位精确、流中与 flush 消费者取消、库整块非有限样本拒绝及公开源入口。正式 `cocobeat-lab resample-audio` 的 19 项检查通过，包括 WAV/FLAC/MP3/Ogg、14 个正常文件、4 个错误后的半成品清理和已有文件不覆盖；独立 API 的 7 项检查通过，确认取消后仅调用一次消费者，末尾 FLAC 校验或 NaN 失败即使已有临时块交付也仍返回错误

原创 64 秒音乐的 48 kHz 输出为 3,072,000 帧，与原 PCM16 独立转换结果逐字节相同，SHA-256 保持 `bd88f48a3b7b641e621a716d7b122d8abd326a6a51177eb06a3f2296896d3538`；由该音乐经开发期 FFmpeg 派生的 44.1/96 kHz 输入也各输出 3,072,000 个有限帧。正式 debug lab 三次耗时分别为 1.134/22.608/24.374 秒，派生输入的往返转换不声称无损，FFmpeg 不进入产品路径

独立 release API 探针对 600 秒、44.1 kHz 静音输入输出 28,800,000 帧，全零且每次回调不超过 1024 帧，耗时 12.492 秒、进程树峰值 RSS 32372 KiB；1 Hz、97 源帧的完整 flush 边界输出 4,656,000 帧，耗时 1.873 秒、峰值 RSS 78228 KiB。High 保留最多 96 个源帧，其 1 Hz flush 会在库内部一次生成最多 4,608,000 个输出帧，分块回调不能降低这个内部缓冲；这些是单次本机观察，debug lab 与独立 release 探针不能合并作发行性能结论

一个独立 libvorbis 样本的原始输入、EOS、Symphonia 为 48001 帧，FFmpeg 输出 47873 帧；公共前缀在零移位时最大差 1.043e−7，Symphonia 额外末尾 128 帧全部为零，本例记录为解码器尾部保留差异，不调整期望帧数或声称任意 Ogg 的双路输出一致

完整源码、依赖、二进制、命令与产物身份见 `target/resample-integration-20261003/evidence.json`，CLI 与 API 明细分别为 `results-cli/result.json`、`results-api/result.json`。重采样合成频响与抗混叠证据继续见 [隔离质量检查](canonical-audio-probe.md#2026-10-03high-合成质量-pass十分钟编码-fail)；Windows/ARM 执行及听感仍为 NOT RUN，标准 Ogg 编码、最终回读和 SongPackage Ready 尚未接入

## UI、雨夜场景与菜单主控

2026-10-03 首批界面采用柔和玩具主体、雨夜空间层次和关键同步强反馈，Ready 与暂停页将主操作和“双人输入”分开，标题下显示菜单主控与接管方式，P1/P2 设备卡和运行 HUD 为场景保留清晰区域；继续复用同一 Logo 停靠、13 个语言变体、Noto Sans、设置草稿与显示预览契约

双手柄与键盘＋手柄共用独立于 P1/P2 的菜单主控，首次按键仅领权，副控 Enter/Start 仅接管，普通副控方向与确认不抢焦点；目标设备可以应答绑定而不转移主控。实际获准暂停的设备接手菜单，任一已加入手柄断线请求暂停，主控断线释放控制权；归中与持键释放屏障继续生效，设置草稿和预览不会因接管被确认或丢失，设计依据与细则见 [输入说明](platform-input.md#交互设计依据)

新增真实 Bevy First/PreUpdate 输入回归覆盖两种键盘＋手柄玩家分配、双设备同批菜单操作、接管、绑定与断连。复查修复音频回调前捕获的旧确认在 Pausing→Paused 后误恢复，以及 Starting/Pausing 期间先修改输入侧设置状态的问题；旧阶段队列在读取音频回调前过滤，输入侧过渡门控保留释放与断连处理。打开设置同帧断连还需保留 Settings(Open) 协议事件而取消 Confirm，失败复现保存在 `target/controller-disconnect-settings-repro.log`，修复后输入 14 项检查通过

参数网格生成双耳 P1、单冠 P2、几何五官、雨夜街道、湿表面与三盏灯；本地 Hit、已确认 Free Sync、Anchor Sync 和 Miss 使用不同表现，下一 Anchor 来自歌曲真实帧表。新开始、重开与返回主菜单清理脉冲，Resonance 仅驱动独立招牌材质；资源来源见 `licenses/ASSET_PROVENANCE.csv`。194 个网格实体及测试上限包含隐藏或视锥外对象，不代表 draw call、帧时或 FPS 测量

MenuOwner 改动前的完整 `CARGO_BUILD_JOBS=1 CARGO_NET_OFFLINE=true cargo xtask check` 通过 105 项 workspace 测试，含十分钟 Replay 检查；计数和日志哈希见 `target/ui-redesign-20261003/workspace-log-summary.json`。最终输入改动后执行 `cargo test --offline --locked -j1 -p cocobeat-runtime`，75 项全部通过；`cargo clippy --offline --locked -j1 --workspace --all-targets -- -D warnings`、格式与游戏构建全部退出 0，116 个已记录源码及资源输入前后哈希一致，主批菜单二进制 SHA-256 为 `089a422e9cf3587d6dbb65be0dc40e1ce5b8548a496ea2a102e8668bdb4dab46`，见 `target/ui-redesign-20261003/final-validation.json`

首轮截图实际目检发现设备卡文字为空，虽然进程与几何检查通过，仍记录为视觉 FAIL；为卡片文字补充真实 flex 宽度后，新增 Bevy 文字测量与布局检查覆盖真实 glyph、主控动态更新、400×300 及逻辑 160×120，原失败截图保留在 `first-pass/`。从最终翻译差异提取每种语言 14 个新增或修改值，13 个语言变体逐个执行 `python3 assets/fonts/verify.py --text LOCALE FILE`，全部通过，见 `glyphs/owner/checks.json`；字体覆盖不代表母语翻译验收

场景使用冻结二进制 `cb0789efd4c7384d34f8c1806305b882ec8ea9627ba4f4c5b4dada2616fc42ea` 完成 Local / Free / Anchor / Miss / Approach 与低、中、高、关闭装饰共 9 项 Vulkan 离屏渲染，逐图检查通过，场景源码哈希保持不变；命令、图像、日志与产物身份见 `target/ui-redesign-20261003/scene/evidence.json`。固定反馈样例证明相应状态可绘制，生产事件接线由真实规则历史与 Replay 回归检查覆盖；截图不证明连续动效、音画同步或真实帧率

主批界面取证实际发现 Fault 首屏只有“已停止”，具体原因被放在滚动信息区下方，记录为视觉 FAIL；修复仅将 Fault 的真实 notice 加入顶部标题，信息行仍保留以便极小窗口分页阅读。现有阶段测试增加首屏原因断言，5 项 app 窄测、runtime Clippy 和游戏构建退出 0；116 个记录输入中仅 app.rs 改变，补验二进制 SHA-256 为 `114fd06d8f2b23df9faf77b5aba264019b269391597632bf321e0a25b0b1bf4e`，见 `target/ui-redesign-20261003/fault-title-validation.json`

最终 UI 矩阵共 22 项逐图 PASS：13 语言 Ready、语言列表、设置、双人输入、暂停、完成、故障，以及 180×120 / 400×300 / 1280×800 scale 2 的主菜单最后信息行。21 张使用主批二进制 `089a422e…`，Fault 修正图使用 `114fd06d…`，原失败图、日志和报告保存在 `before-fault-fix/`；没有把旧二进制的画面声明为重新构建后的重复验收。命令、退出码、PNG/日志哈希、每图检查结论及实测几何见 `target/ui-redesign-20261003/ui-qa.json`

GPU 使用 Linux AMD RX 6650 XT / RADV Vulkan；已知 ICU CJK 词边界诊断按每次运行记录，实际字形、换行和选中行检查通过，未将诊断日志表述为零告警。代码终审按 ponytail-review 删除重复输入状态和无消费者的菜单分类后，结论为 `Lean already. Ship.`；总共 31 个通过的 UI/场景静态样例不替代 Windows/Linux 原生窗口、真实双手柄及混合输入、USB/蓝牙热插拔、物理音频、连续动效体验或母语真人校对，这些仍为 NOT RUN

## 完成结果、故障保留与共同反馈

2026-10-03，完成页只统计本局已记录 Hit 和 core 已确认的判定、Free Sync 与 Anchor Sync，结束或故障时缓存摘要，显示时使用当前语言；不把滚动 Resonance 当全局得分。Ready、暂停、结束和故障各自只提供可用动作，未保存时才显示 Replay 保存；正常结束后保存失败仍保持 Finished，故障后保存成功也不覆盖原播放原因

新增真实规则回归覆盖两位玩家各 4 个 Hit、四档判定 `[1,1,1,4]`、2 次 Anchor Sync 与 2 次 Free Sync，Replay 重放事件一致；显示预览中的故障保留草稿并在 16 秒时安全回退。审查发现并修复菜单保存屏障误吞演奏期同批 Hit，以及保存重试覆盖故障原因或把完成变为故障的问题；键盘和手柄保存后 Hit、磁盘路径被普通文件阻塞后重试成功及保存事实回读均有行为测试

合作双环的 Precise / Good 由真实 Anchor Sync 中双方判定决定，双方均 Precise 时使用完整强度，其他成功组合为 0.68 倍 alpha；两档共用半径、开始帧、衰减和持续时间，Free Sync 不受该等级标志影响。共同环从非零亮度短暂增强后淡出，本地 Hit 仍立即反馈；三环九个时间采样的 ECS 检查验证强度、几何、可见性与生命周期

`cargo test --offline --locked -j1 -p cocobeat-runtime` 的 82 项测试、workspace all-targets Clippy `-D warnings`、格式、依赖边界及游戏构建全部通过；124 个已记录源码、配置和资源输入前后稳定，二进制 SHA-256 为 `b373a534a75ed4735f45b3fc531c653379f34f3aca3028ca2aba07c29181787b`，见 `target/followup-20261003/validation.json`。此前 `c08ac33` 的 [Lightweight CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37096598255) 也已通过，29 项轻量测试和缓存恢复/保存成功；该工作流不编译运行时或测 GPU，不能代替本次本地检查

13 个语言变体各新增 13 个结果/故障键并修改开始菜单键，完整键集合和占位符一致；逐 locale 对实际差异文本执行 `python3 assets/fonts/verify.py --text LOCALE FILE` 全部通过，见 `target/followup-20261003/glyphs/checks.json`。这些证据验证字形和软件契约，母语校对、真实设备、音画同步和参与者体验继续单独验收

`--feedback-motion-smoke NEW_DIR` 复用原生 Bevy 离屏截图和生产反馈衰减，以默认 DuoRules 推进实际 Hit 与双方 Watermark；7 个模拟 Hit 在记录帧 9/30/31/62/90 输入，共同 Free / Good Anchor / Precise Anchor / Miss 分别在真实规则确认帧 44/66/96/126 出现，确认前没有共同环。240 张 1280×800 原始 GPU 帧完整保存，双方 Good / Precise 脉冲拥有相同衰减，未来 Anchor 只按预定帧表接近；逐张目检 17 个关键帧通过，原帧另经开发期 FFmpeg 打包为 8 秒、30 fps 的预览视频，不进入产品编码路径

连续反馈取证使用 `b373a534…187b` 的保留二进制，命令、124 个原构建输入、完整帧/日志/视频哈希及逐帧事实见 `target/feedback-motion-20261003/evidence.json`，视频 SHA-256 为 `9401a14f6465a7ba7d4d79a443e8bc660aabc7c327788f8d19cc8278701755d0`。固定 30 Hz 模拟和视频时间轴不代表实测渲染帧率；没有真实输入、音频播放或真人连续观感验收

随后输入复查发现设置页内变为 Finished/Fault 时，底层旧焦点可能对应另一动作；仅修改 input.rs，在阶段变化时重置底层游戏焦点，设置页自身焦点、草稿和主控不变。17 项输入窄测和完整 83 项 runtime 测试通过，补充 Clippy、格式和构建通过；相对上述 124 个输入仅 input.rs 改变，证据见 `target/followup-20261003/focus-validation.json`，该批 UI 二进制为 `7b99f614b583696a0703af6020c49cd94fb34e2fb9d38263e978de2a0ab43bc3`，既有连续反馈不据此声明重新录制

该二进制完成 12 项 GPU 渲染和逐图检查，覆盖中文/英文/德语/法语/俄语完成页、中文暂停/故障/显示预览中故障、三个极小或缩放视口的最后信息行及 400×300 的 Miss 7/7 行；Finished 样例真实调用空输入会话的 finish，7 次 Miss/人来自实际开发 Anchor，未写盘保存。报告为 `target/followup-20261003/results-ui/report.json`，完整长错误、Replay 失败和预览还原提示可读；设置 smoke 将预览时间固定在 1 秒，实际 15 秒回退由上述行为检查验证

最后补齐 Finished/Fault 中重新开始或返回主菜单的共享保存预检，磁盘失败时仍保留当前阶段、结果、原错误和全部 Replay 事实；恢复目录后允许迁移，新增真实文件阻塞与恢复检查通过。目检还发现完成标题的 Anchor Sync 数值孤立换行，13 语言共同统计改为明确两行；最终 84 项 runtime 测试、workspace all-targets Clippy、格式、构建和全部差异字形检查通过，见 `final-validation.json` 与 `glyphs-final/checks.json`，二进制 SHA-256 为 `3ac7ecf1f40f6eba2a3b011a00ff20c3596af0fb0f8954646d7196715ac460e2`

最终二进制对五语言完成标题、三个末行视口和 Miss 7/7 共 9 项补验全部通过，逐图确认共同次数同行、焦点和完整信息可读；报告为 `target/followup-20261003/results-ui-final/report.json`，SHA-256 为 `851352124e09eb656823e1e2f7f4af84e8d7d23c861b2d4ff469e068b8754680`。暂停、故障和显示预览中故障沿用明确标注的 `7b99f614…` 三项画面证据，原 12 图、补验 9 图及各自二进制均保留；最终代码和文档复核为 `Lean already. Ship.`，设备与真人退出条件没有据此勾选

## 混合输入换席与菜单焦点

2026-10-03 复查双手柄及键盘＋手柄时，补齐“解除手柄分配”入口：唯一手柄可从 P1 解除后加入 P2，反向也成立；保留键盘绑定、菜单主控、另一玩家和按键释放屏障。演奏中未加入的手柄不再触发 Replay 保存，取得菜单主控后仍可使用菜单保存

首轮 85 项 runtime 测试通过后，独立复审发现动态解除行在断连后消失可能改变所选操作，已按操作身份恢复焦点；被移除的解除操作返回对应玩家加入入口。新增行为回归覆盖两种换席、同批接管屏障、重按才生成 Hit、未加入手柄保存拒绝，以及断连前后 Join P2／解除 P1／返回的焦点身份；首轮记录保留在 `target/controller-mixed-20261003/validation.json`

最终 `cargo test --offline --locked -j1 -p cocobeat-runtime --lib` 的 86 项测试、runtime all-targets Clippy `-D warnings`、格式及游戏构建通过；依赖边界检查通过，本批未改依赖。最终输入源码及 13 个 locale 的哈希前后稳定，命令和日志见 `target/controller-mixed-20261003/final-validation.json`，游戏二进制 SHA-256 为 `5c70a3be9d563be791f37f366f9cd6d21796301334967ebc054ba55b6421279d`

每个 locale 的两条新增文案和一条冲突提示均通过 Noto 字形与 `{player}` 占位符检查，见 `target/controller-mixed-20261003/glyphs/checks.json`；独立正确性和 ponytail-review 复审均无剩余发现，结论 `Lean already. Ship.`。本批未新增 GPU 截图或物理设备操作，Windows/Linux 双手柄、混合设备和 USB/蓝牙验收仍为 NOT RUN

此前 `66d0771` 的 [Lightweight CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37099092172) 已完成且通过，缓存恢复及保存成功；该远端结果只覆盖对应提交的轻量工作流，不包含本批 runtime 修改

## 严格 canonical 读回入口

2026-10-03，`decode_canonical` 复用有界源解码与 Ogg 页校验，以调用方提供的编码输入帧数验证最终文件；仅接受 Ogg Vorbis、48 kHz、恰好双声道，输出有限 PCM，并验证 CRC、EOS、帧 0 连续性和最终实际帧数。既有源入口仍允许单声道复制和其他支持格式，严格入口不做该转换；所有回调块在整次成功前仍是临时结果

`cargo test --offline --locked -j1 -p cocobeat-media` 的 8 项测试、media/lab all-targets Clippy、lab 构建及格式检查通过。新增 4,800 帧原创双频立体声样本验证左右声道、样本时间坐标和消费者取消；错误容器、单声道、错误采样率、越界期望、CRC/截断及伪造 EOS 均被拒绝。首轮负例对失败发生时机的测试预期已修正，原失败日志保留，产品代码未为测试绕过校验

冻结 lab CLI 完成 21 项实际调用检查，严格成功输出与同一文件的源解码输出逐字节一致；另以较长 Ogg 伪造末页 EOS 并修复 CRC，确认已经写出临时 PCM 后仍会因最终实际帧数不符而失败，并删除本次半成品；已有输出原字节保留。完整命令、结果及源码/二进制哈希见 `target/canonical-readback-20261003/validation-summary.json` 和 `cli-results.json`，冻结二进制 SHA-256 为 `71c6503d1594606c2a35ef5bae1fbbf6aab45309b8f567db6a99d767308c2e11`

上述是最终文件的结构、时轴和有限性软件检查，不能证明编码前后瞬态完全重合或音质达标；生产编码器准入、seek、曲库听感、跨平台读回和 SongPackage Ready 继续按 05 验收。独立审查为 `Lean already. Ship.`，未添加产品依赖

## 单边频谱 HFC 后续候选

隔离工具 `tools/mir-spectral-probe/` 复用 oxifft 的 Hamming/FFT 与 OxiMedia 的 HFC onset API，固定 128 帧窗、64 帧 hop 和 1.5 倍局部门槛；以实际 PCM 窗支持区间确定中心坐标，补实际尾窗，不平移预测或改真值。等能量不同频率的单边 HFC 比值为 4.19990，完整镜像谱对照几乎相同，验证单边频率加权确实生效

2 项软件测试、Clippy、格式、构建及独立指标复算通过；原 10 声道和 3 个控制通过，另 5 个控制仍 FAIL，保留首帧/单样本漏检与噪声误报。固定 [观察清单](../testdata/synthetic/mir-spectral-probe/observations-20261003.json) 记录完整矩阵与源码、产物身份，复现入口见 [工具说明](../tools/mir-spectral-probe/README.md)；原两批工具和观察保持不变，没有启用生产 MIR，最终编码回读、真实音乐及人工标签仍待验收

## 最终音频对象 staging

2026-10-03，`prepare_canonical_audio` 与 lab `prepare-audio` 把最终 Ogg 有界复制到新建目录，记录实际写入字节的 BLAKE3 和长度，落盘后严格读回同一副本。已有目录不会覆盖；失败只清理本次创建的音频和空目录，源文件保持，成功时也只有 `song.audio.ogg`，完整 SongPackage manifest、分析/谱面与原子 Ready 尚未创建

新增 3 项行为测试覆盖精确副本身份、已有目录保护、期望帧数/源文件/512 MiB 上限和失败清理，media 共 11 项通过。本轮工作区完整 `CARGO_BUILD_JOBS=1 CARGO_NET_OFFLINE=true cargo xtask check` 通过 125 项测试，包含并行菜单实现的 88 项 runtime 测试；格式、Clippy、依赖边界及 game/lab 构建通过，137 个已记录源码、资源和样本输入前后稳定，记录见 `target/staging-ui-20261003/recheck/validation.json`

冻结 lab 二进制 SHA-256 为 `2f1d2198d76f26674e5e8063e545a311ccc7863f62c023c6ae5f27d7c7b1fe12`，27 项真实 CLI 调用全部通过：三次准备结果一致，两次严格读回、8 项期望帧数拒绝、9 项输入拒绝及 5 项已有内容/路径保护均符合预期。5245 字节样本得到 BLAKE3 `5dc59c4887fd564ce500c9f9eabd07e5f0fdc85c847662d77098899e03a47e2d`，单测与库的一次性 hash 比较，CLI 独立核对格式、确定性及副本字节，没有另写哈希算法

首两轮脚本对 CRC 与截断错误的自由文本和检测层做了过窄假设，产品均已正确拒绝并清理；失败报告保留，最终依据非零退出、非空错误、完整清理与原文件保留判断。最终报告 `target/audio-staging-20261003/cli-results.json` 的 SHA-256 为 `559d642234fa75e0a7efc8061afe9f63863b1173564c4a84562ff617450c0f98`，独立复审为 `Lean already. Ship.`。本批未新增第三方包版本，schema 保持仅标准库；编码音质、完整包事务和跨平台执行仍待后续验收

随后仅修改 runtime/view 的滚动标记也使链接 runtime 的 lab 重新构建，最终 lab SHA-256 为 `73b13c5994384e37a8a885f635a13d0d730a5788284be0785f1216481de71835`；对新二进制补执行准备成功、严格读回和已有目录拒绝三项检查，字节、BLAKE3 及 PCM 与原矩阵一致，原 27 项证据不覆盖。media/schema/lab/Cargo 源码身份保持，补验见 `target/audio-staging-20261003/scroll-followup.json`

## 全频带与谱通量固定对照

全频带诊断从不可变提交 `e1b3a26` 构建基线与仅改两个 setup 字节的扩带副本，19 例固定输入完成 34 次编码及严格双路完整回读、4 次原 guard 拒绝，原 10 例基线 Ogg 逐字节重现。完整长度、每秒、首尾、峰值邻域与频段诊断保留质量改善和退步，不据整体 SNR 遮盖局部退步；独立审查和错误路径检查通过，见 [固定观察](../testdata/synthetic/canonical-audio-probe/fullband-observations-20261003.json)，正式 vendor/profile、输入域与生产准入未变

谱通量对照在首次生成/检测前冻结新增音色控制，3 项软件测试、格式、Clippy 和 release 构建通过；独立复核重建 7 个 PCM，复算 50 次分析、88,834 个窗口及 48 项离散评分。HFC 完全复现旧 13 PASS / 5 FAIL，Flux 旧矩阵为 11 PASS / 7 FAIL，新 6 项两者各 1 PASS / 5 FAIL；连续渐变只记录 HFC 0 峰与 Flux 125 峰，不纳入 F1。声明、首次 JSON 数值比较失败、修正、完整质量 FAIL 和来源身份均保留，见 [观察清单](../testdata/synthetic/mir-flux-probe/observations-20261003.json)

两批均未播放音频、引入真实音乐库或人工标签，也未接入生产编码/MIR；听感、seek、最终编码回读分析、Windows/ARM 与设备验收分别保持 NOT RUN。第三方研究台账沿用 88 个版本，新增控制的 PCM 和构造标签为 CC0-1.0，生成器源码为 MPL-2.0

## 菜单焦点与真实阶段等待

2026-10-03，菜单选中描边以 160 ms 亮度反馈收尾，箭头、正文、背景与操作立即更新，语言和菜单主控变化不重新触发该反馈；`Starting` / `Pausing` 共用生产阶段的过渡状态，在面板顶部显示 2 px 短等待标记，真实阶段变化后隐藏。歌曲进度保持直接取 SongTime，装饰不改布局、文字 alpha、输入门控或规则事实

完整检查与音频 staging 共用上节记录：125 项 workspace 测试包含 88 项 runtime，两个新增 ECS 行为检查用手动 Time 验证描边衰减、选中即刻更新、几何及文字不变、等待标记显隐和歌曲进度。首轮 Clippy 参数数量失败已按既有 system 元组方式修复；通过后的游戏 SHA-256 为 `83638be50132a7874f387bb7ca227bb5c24cdb2848b8975f21d7e468a261a5da`

首轮 12 项 Vulkan 离屏截图覆盖 Starting/Pausing/Ready/Paused、中文 1280×800 / 400×300、英文对照及滚动末行，逐图为 10 PASS / 2 FAIL：两张过渡末行图的等待标记随面板滚走，虽然进程和几何检查退出 0，仍按视觉 FAIL 保留。原尺寸和像素复核确认正文从面板上缘内开始，没有进入 Logo 区；既有裁剪保持，失败图和逐图结论见 `target/menu-feedback-20261003/gpu/report.json`

修复只为标记添加 Bevy 原生 `IgnoreScroll`，137 个已记录输入中仅 view.rs 改变；6 项 view 窄测、workspace Clippy、格式和构建通过，见 `target/staging-ui-20261003/scroll-fix/validation.json`。最终游戏 SHA-256 为 `db071c3f37b3739727246e5e5da3e0a8611c96bf80eb3572c555b7dee49e5164`，补验仅重拍两个失败状态及 Ready/Paused 的滚动对照，四张均通过，等待标记固定、按阶段隐藏，Logo 和选中行完整可读，见 `target/menu-feedback-20261003/scroll-fix/report.json`

本批 GPU 为 Linux AMD RX 6650 XT / RADV Vulkan，CJK 词边界诊断继续如实记录；160 ms 连续变化由 ECS Time 检查覆盖，GPU 是静态取样，没有据此声称连续录像、音频确认时延、真实性能或物理设备操作。主控和混合输入规则独立复核无新遗漏，最小修复后的 ponytail-review 为 `Lean already. Ship.`，双手柄、混合设备及 USB/蓝牙实测继续为 NOT RUN

## 画质与必要反馈矩阵

2026-10-03 扩展既有 `--feedback-smoke`，保留双参数用法，并接受 `EFFECT PRESET PNG` 或 `EFFECT PRESET WIDTH HEIGHT SCALE PNG`；新增 ECS 检查确认四档画质只影响呈现设置，同一预设反馈的 pulse、等级、SongTime 和 Resonance 保持一致，错误参数明确失败

11 项 app 窄测、runtime all-targets Clippy 与 game 构建通过，构建输入前后稳定，记录见 `target/quality-feedback-20261003/validation.json`；随后完整 workspace 检查的 141 项测试包含 89 项 runtime。游戏冻结二进制 SHA-256 为 `1fa42d26741d5117e83f1a2e0f94ab2a7bfed2bac4dd3d759c5e652e4a6e42fe`

四档画质 × Free / Good / Miss 共 12 张 1280×800 图，以及关闭效果的三张 400×300 图全部逐张通过；独立核对命令、PNG 尺寸、实际 UI/3D camera 尺寸、所有画质字段和反馈采样，3D 分别为 640×480 / 320×240，HUD 仍按原生窗口尺寸呈现。关闭效果时单环、双环与 P1 Miss 倾斜可辨，小窗口身份标签正常换行；底部仅用于 smoke 的说明覆盖少量环底缘，但环数和玩家身份仍可辨，该限制在报告中保留

完整结果为 `target/quality-feedback-20261003/gpu/report.json`，SHA-256 `12c883f89d3360d629a07e278c39c051a8605e0ca46107dc3c78434fdf8c1563`；冻结二进制前后相同，GPU 为 AMD RX 6650 XT / RADV Mesa 26.2.3 Vulkan，15 次运行均无 WARN/ERROR。这是合成静态状态检查，没有新增真实 core 连续帧、物理设备、音频同步或实际 FPS 结论

## 初始 SongPackage 与手工创作入口

2026-10-03 已实现标准库内容类型、私有有界 Postcard 编码、四对象构建/验证及 lab 的 `build-authored-package` / `verify-package`，完整字段与命令见 [包契约](song-package.md)。实际能量来自最终音频，Anchor / 段落来自带来源说明的手工输入；没有用空壳对象代替未运行的自动 MIR

初版完整 `CARGO_NET_OFFLINE=true CARGO_BUILD_JOBS=1 cargo xtask check` 通过 141 项测试、边界、格式和 Clippy，日志为 `target/song-package-20261003/workspace-check.log`。独立审查随后发现先 hash 原路径、再测量、再 hash 的流程仍允许源文件在测量期间变化，已改为先准备并严格读回自有 staging 副本，再通过一次性 callback 从同一副本测量；新增测试实际替换/删除原路径并验证最终音频和能量不变，也覆盖 callback 失败清理

最终 23 项 media、7 项 schema 和 4 项 lab 窄测通过，workspace all-targets Clippy、格式、边界及 lab 构建通过，构建输入前后稳定；未因局部事务修改重复运行未变的十分钟 Replay 压测。命令和来源身份见 `target/song-package-20261003/final-validation.json`，最终 lab SHA-256 为 `e0c4708a18c91cce0b2978a50b53aa9c1e08c90732a90f47c101d35e1d3e3f26`，独立正确性与 ponytail 复审为 `Lean already. Ship.`

冻结 CLI 的 47 项独立检查记录为 46 PASS / 1 FAIL：两种包均成功构建并通过对象身份检查，64 秒包还通过完整独立能量对照；未知 JSON 字段/版本、越界/乱序事件、期望帧数错误、源与对象损坏、已有目标、符号链接及失败清理等负例均按预期拒绝并保留原内容。检查器要求正常错误退出码 1，不将 panic 或信号终止当作负例通过

64 秒包的独立 Python Postcard 读取器与 BLAKE3 helper 核对实际四对象、规范 manifest 身份、7 个 Anchor、6 个段落和 3000 个能量块；FFmpeg 解码实际包内 Ogg 后逐块重算左右 RMS / peak，预先固定绝对容差 `1e-6`，最大误差分别为 `9.82816e-9` / `4.47035e-8`。包 BLAKE3 为 `8ba83a120c57b51db990c044c4dee77116872d92c282a9a09a6ff59a283bd5df`，该开发编码候选只用于内容格式实验

唯一 FAIL 是 4800 帧短 fixture 的默认 FFmpeg 读回仅输出 4672 帧，原门槛和失败保留。诊断表明共同的前 4672 帧与 Symphonia 最大差 `7.45058e-8`，没有生产起点平移；显式 `-flags2 +skip_manual` 得到 4928 帧，其前 128 帧为额外前滚，余下 4800 帧与 Symphonia 一致，末包 side data 报告 `discard_padding=128`。独立原始正弦也确认 Symphonia 尾部是实际内容而非补零；这不阻断新包的格式和事务实现，但该 fixture 的默认 FFmpeg 跨解码器帧数检查仍为 FAIL，未修改产品时间轴或放宽比较门槛

完整报告、原始矩阵和短样本诊断分别为 `target/song-package-20261003/cli-report.json`、`cli-summary.json`、`cli-diagnostic-4800/diagnostic.json`；265 项产物封存清单 `cli-sealed.json` 的 SHA-256 为 `3db893174e1516bc263b0e0482d6db4459af37711cbd62207425dab70abcf706`。这批只证明初始内容包的软件行为，Windows/ARM 包事务、完整 MIR/Anchor/舞台、游戏曲库接入、编码音质、听感与设备验收继续单独推进

## 固定归一化 Flux 过滤

2026-10-03 的[隔离工具](../tools/mir-flux-gate-probe/README.md) 保留原生 Flux 的预测坐标和峰选择，仅使用提前声明的局部谱幅值变化比例 `0.5` 过滤。4 项测试、fmt、Clippy、release 构建及独立审查通过，25 个输入的 44,417 个窗口、原生预测和 48 组指标逐项复现；独立 DFT 抽查 96 个实际 PCM 窗验证幅值总和，未改旧 Matcher、真值或时间原点

新增 6 项离散音色控制全部通过，24 项离散控制的额外峰从 537 降为 0；旧 18 项仍为 11 PASS / 7 FAIL，两个持续音此前在容差内的假峰被删除后新增两次首帧漏检，全部失败保留。连续 fade 从 125 峰降为 0，仍不计 F1；实际曲库、慢起音、叠加声部、近邻强弱事件和最终 Ogg 回读尚未验证，整体质量为 FAIL，未接生产 MIR

紧凑[观察清单](../testdata/synthetic/mir-flux-gate-probe/observations-20261003.json) SHA-256 为 `f5bf5cca255bf31353671eef32a1c7216ae9a57e19387733f940da383f58199b`，完整窗口报告保留在 `target/mir-flux-gate-20261003/results-v1/report.json`；本批没有生成新 PCM 或新增第三方版本，研究台账仅补相同 37 个包的使用方

## 数值域与全频带组合检查

2026-10-03 的[独立诊断](../tools/canonical-audio-probe/numeric-fullband/README.md) 仅编码四个冻结的 sign-kernel 源一次，每例执行严格与 FFmpeg 完整回读，均为 48000 帧，最大解码差 `4.76837158203125e-7`；原 PCM 身份、首尾、逐声道 SNR、峰值与频段误差全部保留。已核对实际 High 系数表的 48000 相位证书及有条件的 codec 算术预算，14 个插桩阶段实际有限；独立复审复算证书及 704 个质量指标，通过后保持正式候选和生产 guard 不变

源码 hash 分隔换行检查失败及外层进程退出 143 都发生在编码前，原日志保留，最终四次编码没有扩大矩阵。上采样率及固定 96/192 kHz 的核界只适用于实际源峰值不超过 1 的声明条件，编码包络 4 不是生产配置；仍有明显残余失真与局部退步，质量准入、seek、其他原生平台与听感均未因此通过。工作区复现需要两批冻结 target 证据，缺少完整历史生成器的 fresh checkout 复现边界已明确写入工具说明

数值域组合的 171 项冻结产物清单 `target/numeric-fullband-20261003/FROZEN.json` SHA-256 为 `0bfa350ac218f6448aa7259ad9061f7025a96bd780e07ad5261402c26c65961a`，持久[观察清单](../testdata/synthetic/canonical-audio-probe/numeric-fullband-observations-20261003.json) SHA-256 为 `af49b1feede15ae0eb7164d9506e34ab82cf810d5fb318e3c3362882f97abe6d`；最终独立复审为 `Lean already. Ship.`

## 手工歌曲包运行时闭环

2026-10-03，游戏新增 `--package DIR`，使用包的最终 PCM、实际总帧数、手工 Anchor 与完整 manifest 身份；无参数保留开发歌曲，普通包启动沿用品牌开场、Ready 独立确认和原有输入门控。读取、哈希、CRC 与解码绑定同一份有界音频字节，重开共享 Kira PCM，未知规则明确拒绝；SectionCue 与能量尚未参与舞台呈现，完整使用见 [包契约](song-package.md#运行时加载与会话)

`cargo test --locked --offline -p cocobeat-media -p cocobeat-runtime` 的 122 项测试通过，包含 25 项 media 与 97 项 runtime；真实短包经 Kira MockBackend 的完整输出逐帧等于加载 PCM，左右声道保持区别，结束后 clone 重播从零开始。Session 检查覆盖 1 帧、4801 帧、超过 64 秒及十分钟边界，HUD 检查覆盖短歌、90 秒、首尾越界与未加载状态；独立复查发现的品牌显露期间零时长 HUD 和奇数帧预览中点不一致均已修复

最终 `CARGO_NET_OFFLINE=true CARGO_BUILD_JOBS=1 cargo xtask check` 通过依赖边界、格式、workspace all-targets Clippy 及 152 项测试；`cargo build --locked --offline -p cocobeat-game -p cocobeat-lab` 通过，174 个构建输入前后保持一致，547 项第三方版本与台账一致，未新增第三方版本。命令与哈希见 `target/package-runtime-20261003/validation.json`，游戏 SHA-256 为 `1dc74520b53098f99903be8313f8e1df59af7a252870213ae4514d6f5a5c6381`，独立正确性与 ponytail 复审为 `Lean already. Ship.`

冻结游戏的 49 项 CLI 检查全部通过，独立 Postcard/BLAKE3 读取器给出包身份和 Anchor oracle；覆盖短包、64 秒包、相同音频不同谱面的双向 Replay 拒绝、未知规则、最后合法帧与负值/EOF/越界 Hit、损坏/缺失/符号链接对象、参数错误及旧开发 Replay。变谱面和未知规则夹具由上轮冻结 lab `e0c4708a18c91cce0b2978a50b53aa9c1e08c90732a90f47c101d35e1d3e3f26` 实际构建，正常错误必须退出 1，panic 或信号不算负例通过

两张 1280×800 Vulkan 离屏图逐张检查通过，实际 HUD、50% 进度及 Anchor 预告分别来自 4800 帧短包和 3,072,000 帧长包，`CONTENT_SAMPLE` 与独立包数据逐字段一致；短包中点为 0.05 秒，HUD 一位小数显示 `000.1 / 0.1`，不能据此认为进度已满。GPU 为本机 RX 6650 XT / RADV，预览中的反馈为合成状态，不代表真实输入或音频驱动的画面

最终报告为 `target/package-runtime-20261003/qa-report.json`，238 项证据路径及哈希清单 `qa-sealed.json` 的 SHA-256 为 `2871e9cacd16327d9ce543218317927759a9dbad5e9a0b01fddfbc86514d1f86`；真实音频输出、物理键盘/手柄、听感和 Windows/ARM 运行时验收仍为 NOT RUN，上轮短 fixture 默认 FFmpeg 的 4672/4800 帧互操作 FAIL 保留，未被本次运行时结果改写

## 手工段落表现与混合字体

2026-10-03，包内 SectionCue 接入运行时字幕与原生三维门框，严格选择未来提示，同帧取最高 ID；没有未来提示时保留最近提示，负时间及歌曲 EOF 清空。门仅在未来六秒内显示，使用歌曲游标确定位置，暂停保持位置，菜单隐藏辅助字幕；Ready、重启等待及结束不保留旧提示，判定、Replay 与 Anchor 事实保持独立，完整接口见 [歌曲包](song-package.md)

`cargo test --locked --offline -p cocobeat-runtime` 的 103 项测试通过，覆盖 cue 边界、同帧、内容身份、菜单与结束清空、暂停门位置、长文本裁剪和真实 TextPipeline 字形排版。六份已有 Noto 使用 Fontique 原生回退并保留地区主字体，测试包含字体晚加载、字体集重建、非持续脏标记、CJK 中的乌克兰语和英文界面的 CJK；初次测试编译误用 FontData 的 `blob` 字段，修为实际公开 `data` 后通过，失败日志仍保留

workspace all-targets Clippy、格式、依赖边界与 game/lab 构建通过，140 个构建输入前后哈希一致；新增 Fontique 直接依赖使用主版本范围，解析版本仍为已有的 0.9.0，第三方版本总数仍为 547。当前游戏 SHA-256 为 `c27609700b05615ff38a87ac61de6a80712f001f21d097e2254896bc863f1f5b`，lab 为 `70d893d2f5ab3e11afa1a96c5c8cfc33fcf90b80acc292128abbfca7cef9670a`，源输入、命令和日志见 `target/section-runtime-20261003/validation.json`；本批只重跑相关 runtime 测试，上一批 152 项完整 workspace 结果按原源码保留

冻结 lab 实际构建三份只修改标签的包，独立 Postcard/BLAKE3 读取器复核原始数据和完整身份；13 项 CLI 全部通过，覆盖帧范围、整数与溢出、语言、画质、视口及参数错误，以及各包自身 Replay 通过、相同音频不同 cue 包双向 Replay 拒绝。256 字节标签中的换行和制表符仅在显示时折叠，`{label}` 保留字面值，包内原文不变

10 张 Vulkan GPU 图已逐张检查通过：21 秒远门、23 秒近门与独立 Anchor 标记、24 秒到达清空、400×300 关闭装饰保留必要门框、英文 UI 中的中日韩标签、中文 UI 中的乌克兰语、最长标签裁剪、61 秒最近提示、64 秒 EOF 以及原有中点预览；三张混合字体图另经独立复核。场景现为 197 个固定网格实体，不按帧创建门；数量检查不代表帧时间或性能预算通过

批量执行在前八张完成后收到退出码 143，原因未知，确认无存活子进程后只补执行尚未完成的 EOF 和中点两张，原图与中断记录保留。四组 CJK 日志共 13 条既有 ICU 分词模型诊断仍保留，实际字形与裁剪通过不等于日志零告警；已知稳定版限制见上文国际化记录

最终 `qa-report.json`、`qa-visual-review.json` 与 105 项 `qa-sealed.json` 均在 `target/section-runtime-20261003/`，封存清单 SHA-256 为 `e98012428eeb9e0c6b7366b5bb5fe08d359275ea80f7e66179fe33ace26929de`，主线程已逐项复核哈希。正确性与 ponytail 独立复审为 `Lean already. Ship.`；本批静态画面、排版与软件检查不替代真实音频、物理手柄、连续动效、性能、Windows/ARM 运行时或真人双人体验，这些仍为 NOT RUN

## MIR 近邻与弱声部控制

2026-10-03，同一[谱变化过滤工具](../tools/mir-flux-gate-probe/README.md#近邻慢起音与叠加声部)在运行前冻结九项各一秒的构造控制：四种强弱近邻、两种慢起音、等幅叠加、弱声部叠加及弱声部独奏；128 帧窗、64 帧 hop、原生峰选择、归一化门槛和 ±480 帧 Matcher 不变。384 帧近邻的匹配窗重叠，保留最早可行的一对一配对，不能用匹配数量推断物理声部；慢起音的 truth 和 metrics 为 null

隔离 manifest 的 5 项测试、fmt、Clippy 和 release 构建通过，工具实际 `--controls target/mir-next-controls-20261003/controls-v1` 及旧报告回归均因质量 FAIL 退出 1。新七项离散控制为原生 2 PASS / 5 FAIL、过滤后 4 PASS / 3 FAIL，额外峰 229 → 0，漏检 2 → 3；两项慢起音的 97 / 95 峰均过滤为零，只作观察。旧 25 项所有 44,417 窗在仅去除计时字段后完全重现，累计 31 项离散控制为 21 PASS / 10 FAIL，未准入生产 MIR

逐窗证据区分两处问题：384 帧近邻弱峰被原生局部均值门槛压制，过滤前后没有差别；弱叠加在 frame 24000 确有原生候选，谱变化比例约 0.13956，被固定 0.5 门槛删除，而同一弱声部独奏比例约 1 并通过。原 Matcher 曾把较早的 23552 假峰匹配给 24000，原始配对和实际候选均保留，不能把过滤前的匹配数当成已正确恢复该声部

独立检查从实际 f32le 重建全部 432000 帧并逐位匹配，用 NumPy f64 FFT 与数学 Hamming 复算 6741 窗，谱通量最大绝对差约 1.15e-5、归一化比例最大差约 4.87e-7；全部原生候选上的过滤去留及 Matcher 字段一致，未宣称跨 FFT 的 f32 位一致或独立复现微小噪声的原生选峰。新增声明与生成器已有来源台账，无新增依赖版本，软件验证与独立复审为 `Lean already. Ship.`

命令、日志与冻结输入见 `target/mir-next-controls-20261003/execution.json`，独立数值检查见 `independent-controls.json`；执行器准备阶段发现 `/usr/bin/time` 不存在，尚未生成 PCM，随后直接调用工具并保留该记录。当前工具 SHA-256 为 `21188dee93996d16dfa66f3400fea3350f2e512f55e9423684fb07419bf04cf2`，完整报告为 `controls-v1/report.json`，SHA-256 `ac90ae616cb00169a1c3b4e8bae83850b419114ae3e8da05509d27a508b2d232`；持久[观察清单](../testdata/synthetic/mir-flux-gate-probe/observations-next-controls-20261003.json) SHA-256 为 `1e70786c4a8639a1eef10c4b32c2fd8ab269d12a5beb8a8829c6ba1341b19616`，真实音乐、最终 Ogg 回读、人工标签、beat/downbeat、置信度和 Anchor 可玩性继续为 NOT RUN

## 手工区间驱动的确定性轨道

2026-10-03，新增只依赖 schema 的 `cocobeat-stage`，从真实 analysis 区间生成直道/广场，以整数 SongTime 采样距离和路宽；runtime 共享同一计划，用固定五张地面网格呈现区间和空隙，终点来自实际歌曲长度。Anchor 与 SectionCue 保留独立的真实时间，原四对象包与 Replay 格式不变，lab 增加 `inspect-stage PACKAGE FRAME`，完整契约见[歌曲包](song-package.md#内存-stageplan-与整数采样)

`cargo test --locked --offline -p cocobeat-stage -p cocobeat-runtime -p cocobeat-lab -p xtask` 的 115 项相关测试通过，其中 runtime 104、stage 3、lab 4、xtask 4；全 workspace all-targets Clippy、格式、依赖边界及 game/lab 构建通过。147 个构建输入前后哈希一致，新增本地 crate 未增加第三方版本，仍为 547 项；游戏 SHA-256 为 `a3a556d9a29dc86fb1a8c84be5a7f8fe5fc1d11f150d37702af93a76ae96362e`，lab 为 `0b4ddf59eb1c7c77f9ed4a1fe9f96ccedb3724c3a429eb0b31b0e24a6d0c17a2`，源输入与实际命令见 `target/stage-runtime-20261003/validation.json`

复查修复了密集区间回退遗漏首尾截面造成曲外拓宽，以及音频游标短暂越过 EOF 时终点显示不稳的问题；原始 SongTime 与判定事实保持，只有呈现游标限制在歌曲范围。首轮 Clippy 要求原生 `as_chunks_mut`，替换后重新通过全部上述检查，原失败日志保留。固定 257 截面预算会近似极密区间的细小轮廓，完整计划不截断；短区间的拓宽按长度收敛，未据有限网格数量宣称性能验收通过

冻结二进制的 36 项 CPU/CLI 检查通过：真实 0.1 秒、64 秒和 600 秒包、空段落、段间空隙、独立 cue 变体、首尾与整数峰值均对照独立 Postcard/BLAKE3 读取器和整数 oracle；重复检查输出逐字节一致，负数、越界、溢出、非整数和缺参正常拒绝。原包及开发 Replay 与既有输出一致，改 cue 后的完整身份错配仍拒绝；全部音频复用已有 canonical 字节，没有新增编码质量结论

14 张 Vulkan 离屏图已逐张目检，包含拓宽与收回、空隙上的独立提示门、关闭装饰的小窗口、DPI、短歌与 EOF、十分钟歌曲末段，以及旧/新默认开发场景对照。默认场景的 PNG 字节和 1,024,000 个 RGBA 像素完全相同，新增 `stage: null` 之外的采样字段一致；包场景为固定 198 个网格实体。实际报告见 `target/stage-runtime-20261003/qa-report.json`，该英文标签矩阵的日志检查不表示既有 CJK ICU 诊断已经修复

0.1 秒极限夹具的最后 Anchor 与终点只差 1 帧，两者的中央横线在静态图中几乎重合，侧括号仍可见；该局限保留，不能据几何正确性认为实际反应时间与预告可读性已经通过

204 项产物的 `qa-sealed.json` SHA-256 为 `3e8a44206a8f5251bb29b075ca442193e361490f5aa0a63f9803470c0dc1bd41`，主线程已逐一核验；独立正确性与 ponytail 复审为 `Lean already. Ship.`

本批覆盖软件行为和静态呈现，完整舞台组合、跨编译版本视觉 Replay、连续帧性能、Windows/ARM 运行时、真实音频/手柄及真人双人可读性仍待对应验收；原路线图的完整退出条件保持

## MIR 分频候选的退化记录

2026-10-03，现有工具新增单一 `--band-candidate`，将同一 128 帧 Hamming / 64 帧 hop 的正谱通量按六个固定频带归一化，再用固定 0.5 门槛和局部极值选择；复用全部既有 PCM、真实窗支持坐标和 Matcher。运行前声明及所有 34 项输入身份已冻结，没有重新生成音频、筛选输入或运行后扫描参数，详见[候选说明](../tools/mir-flux-gate-probe/README.md#分频带局部变化候选)

6 项软件测试、fmt、隔离 Clippy、release 构建及独立审查通过；唯一正式全矩阵运行因质量 FAIL 退出 1。31 项离散控制为 17 PASS / 14 FAIL，665 个额外峰，6 项旧 PASS 退化；3 项连续控制保持不评分。两项近邻控制通过，弱叠加虽然恢复了 frame 24000 的候选，仍有 35 个额外峰，因此修复未通过，旧全谱过滤的 21 PASS / 10 FAIL 结果继续保留

旧基线 51,158 窗逐项完全复现，独立 f64 FFT 进一步复现全部 34 项新预测列表与 665 个额外峰。440 Hz 持续音的高频带和 9973 Hz 持续音的低频带暴露确定性窗旁瓣被小分母放大，不能将问题归结为浮点舍入噪声；真实弱进入的频带幅值与旁瓣证据分别保留。下一步只增加有明确局部频谱尺度的分母下限，保持原完整回归集和门槛，不将这批已用于设计的控制称为未见测试集

执行证据为 `target/mir-band-local-20261003/execution.json`，工具 SHA-256 为 `76ac3ed483985d69c20907b8e4047327b888a6c4b78952154560a6a06c4a2f9a`；[持久观察清单](../testdata/synthetic/mir-flux-gate-probe/observations-band-local-20261003.json) SHA-256 为 `d986c52e3c6acd952a49016123080f868c2b71a8ee60b3a8de43fd94b6db752f`。独立复算、失败定位和最终审查均在同一证据目录，复审为 `Lean already. Ship.`；没有新增依赖版本或新音频，生产 MusicAnalysis、真实音乐、最终编码回读与人工标签仍未通过本批验收

## 精确 Anchor 编辑与原字节保真导出

2026-10-03，`cocobeat-editor` 和 lab 的 `edit-anchors PACKAGE PATCH NEW_PACKAGE` 已接通已有歌曲包的精确帧增删移动、1024 步增量撤销重做与事务导出。补丁绑定完整源身份，操作全部成功后才写新包；实际修改保留音频、analysis 和原 cue，重新计算 chart/manifest 身份，无变化则四对象原字节与包身份完全保留，具体用法见[内容编辑](editor.md)

`cargo test --locked --offline -p cocobeat-editor -p cocobeat-media -p cocobeat-lab -p xtask` 的 41 项测试通过，另运行 `cargo test --locked --offline -p cocobeat-runtime content::tests` 的 6 项加载/播放内容测试通过，合计 47 项相关测试。全 workspace all-targets Clippy、格式、边界与 game/lab 构建通过，150 个构建输入前后相同；新增本地 editor 未增加第三方版本，仍为 547 项。二进制、输入和命令见 `target/editor-runtime-20261003/validation.json`，game SHA-256 为 `950f52c4d14dcad49ee9989752c6bf0095180ed001d2c2653af8d80fecbe2f1e`，lab 为 `1c79d2720861fae90ca64b99b9964561110232e92c65664c01101f784a18b712`

独立审查修复了源包内部输出目录会引入第五项、破坏原包的问题，现在创建 staging 前按规范化父目录拒绝源内路径，包含符号链接父目录别名。媒体窄测实际覆盖合法非规范 Postcard 字节的无变化导出、首次已验证 manifest 原字节快照、后续读取对象与音频副本的身份绑定、失效源身份、非法 Anchor、失败清理和已有目标保留；初次快照之后外部 manifest 被换掉不要求丢弃已验证快照，导出的所有对象仍必须与所使用快照一致

冻结二进制的 35 项 CPU/CLI 用例通过，共执行 36 条产品命令，包含 0.1 秒、64 秒与 600 秒真实包的编辑、四种无变化结果、重复导出、1024 操作及恰好 1 MiB 边界，错误补丁与七种目标失败均保留原内容。独立 Postcard/BLAKE3 读取器核对实际新 Anchor、原 cue/rules、原字节及新身份；三个源包始终只有原四文件且哈希不变，原包与撤销后包的 Replay 输出相同，真实改谱后的包拒绝旧身份 Replay，StagePlan 除完整身份外的几何采样保持一致

唯一 Vulkan GPU 图将首个 Anchor 从 26 秒移到 27 秒，在 26 秒实际加载新包后显示前方预告；`CONTENT_SAMPLE` 的下一 Anchor 为 27 秒，原下一 cue 仍为 40 秒，整数舞台采样与独立包事实一致，灰色括号不遮挡角色身体。原始命令、对象身份和截图在 `target/editor-runtime-20261003/qa-report.json`；这项静态改谱验证不证明物理按键、音频延迟或真人可玩性

182 项产物的 `qa-sealed.json` SHA-256 为 `22e2cd2dfa91f5c92924199d3e7e4e2e2ed048f3790b29004715cd95b823f1b7`，主线程逐一核验通过；独立正确性及 ponytail 复审为 `Lean already. Ship.`。并行舞台下一版在本次二进制冻结后继续修改，提交前另将全部 150 个构建输入与暂存内容逐一比对，避免把下一批源码混入本次验收

本批只交付实际可用的编辑内核与 CLI，波形时间线、候选证据界面、Replay JSONL 诊断和正式菜单接线继续按 09 推进；Windows/ARM 原生导出、真实音频/手柄和真人体验尚未由这些结果验收

编辑提交 `64904ac` 随 MIR 研究提交 `695b3a1` 已推送至 main；后者的 [Lightweight CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37115851761) 通过 60 项 schema/core/replay/media/stage/editor/xtask 测试并恢复缓存，未编译 runtime/GPU。150 项冻结构建输入与编辑提交逐项吻合，交付记录在 `target/editor-runtime-20261003/delivery.json`，SHA-256 `a8b1fe355f3a25bb4023fd343f37046f594d4ba523980a47878789f295c60ee0`

## MIR 频带分母下限候选

2026-10-03，在既有六频带和局部选峰上，仅加入固定幅值比例 `beta = 0.01` 的分母下限 `max(D, band_bins × beta × local_peak_magnitude)`，复用原 FFT，不增加 PCM、依赖或绝对音量门槛。运行前审查删除了一处重复声明字段后重新冻结，正式[声明](../testdata/synthetic/mir-flux-gate-probe/declared-band-floor-20261003.json)、实际执行器断言及原始落盘副本的 SHA-256 均为 `8cdf238f1428af2f1dd9ebda5f4735d98c6b618fdf60fbf35530f5c9236283c3`，参数与输入未在运行后修改

7 项软件测试、fmt、隔离 Clippy 和 release 构建通过；唯一一次完整 34 项运行因质量 FAIL 退出 1。31 项离散为 21 PASS / 10 FAIL，额外峰 665 → 269、漏检 4 → 6；3 项连续控制仍不评分。两项 384 帧近邻和弱叠加均通过原门槛，持续音内部旁瓣假峰清零；旧分频候选的 6 项退化恢复 3 项，noise burst、kick、snare 仍失败，原 7 项边界失败继续保留，因此目标修复和生产准入仍 FAIL

局部分母改变也会改变极值位置，kick 出现的 21 个新坐标已明确记录，未把新预测伪称旧候选子集；新增两次漏检来自持续音内部假峰不再被容差误配到首帧。三个旧基线的 51,158 窗、四路 Matcher 及输入身份完整复现，独立 f64 FFT 的全部 34 项新预测列表一致；有实际 floor 生效的持续音窄测还核对共同增益缩放与 E 不增性质

正式运行命令见 `target/mir-band-floor-20261003/execution.json`，软件日志为同目录的 `tests-final.log`、`clippy-final.log` 与 `build.log`；实际工具 SHA-256 为 `b20305e200311b88c1351c0adb4763c2b9e0bacd38466b3d09a02a3075a0d9f7`；[持久观察清单](../testdata/synthetic/mir-flux-gate-probe/observations-band-floor-20261003.json) SHA-256 为 `96f85582b251735927937a763467073da6b67164e8ff48da480b6168a3ebd0e8`。独立复算与 ponytail 复审通过，结论为 `Lean already. Ship.`；后续依据剩余噪声与衰减结构加入时间背景门控，同一完整矩阵继续评分，真实音乐、编码回读、标签与生产 MusicAnalysis 不因这批软件结果获得验收


## 缓弯、低桥与同一轨道上的预告

2026-10-03，StageCompiler v2 已将真实手工包接入直道 / 广场 / 缓弯 / 低桥、两个固定装饰拱门与实际终点。长分析区间按时长固定编排，整数位置和切线由 SongTime 决定，镜头朝向保持固定；没有从区间标签推断音乐强度、生成新 Anchor 或改变规则输入

初版冻结源码执行 `cargo test --locked --offline -p cocobeat-stage -p cocobeat-runtime -p cocobeat-lab -p xtask`，121 项通过，包含 runtime 106、stage 5、lab 6、xtask 4；全 workspace Clippy、格式、边界与 game/lab 构建通过。独立 Bernstein 有理数参考的 10,080 组采样、60,480 字段吻合；极值、奇数拆分、十分钟 / 100,000 项混合区间和整数边界均有证据

ECS 检查固定 208 个 Mesh3d、14 个 Mesh 资产、9 个动态条带和两个拱门实例，257 行截面保留曲首 / EOF 及长特征边界；桥体三角不越出真实 Bridge，负预滚与越 EOF 的显示游标冻结到合法端点。地面、Anchor、cue、终点和背景共享同一中心线，画质 / Resonance 不改变关键几何。反馈环、街标和湿地细节的实际变换顶点与路面三角插值比较，包含管厚和最大反馈半径，18 个桥坡 / 接缝时刻保持至少 2 mm 净空

初版 41 项真实包 CPU 检查通过，其中 39 个独立 Fraction 采样覆盖接缝前后 1 帧、改谱、空 / 短 / gap 包与十分钟 EOF，另两项拒绝歌曲范围外的预览。14 张初版 GPU 图自动检查通过，但目检发现 20 秒装饰拱顶遮住前方 cue 横梁，因此本批没有直接按自动 PASS 交付；初版图和失败目检保留在 `target/stage-v2-runtime-20261003/visual-review-initial.json`

修复仅将既有拱门竖向比例从 4.1 调到 5.2，保留脚柱、横向位置、资源数和音乐时刻；最终源码重新通过 5 项场景窄测、Clippy、格式和构建。最终 16 张 GPU 图通过自动核对及目检，含 14 张原矩阵补验和两张定向图：一张令桥顶、cue 与真实 Anchor 同指 20 秒，另一张在 22 秒同时显示下坡、24 秒 cue 和 26 秒 Anchor，三者均可辨认；Precise / Good 光环完整且强度有别，低 / 中 / 高 / off、小窗与 DPI 样例均保留

两种无参数开发画面与上一版冻结程序逐像素及 PNG 字节相同，旧、新程序加载相同包的 core Replay 输出相同；合计 42 个 CPU 用例、43 条 CPU 产品命令，跨舞台版本视觉 Replay 仍未实现。最后仅拱门高度与注释发生源码变化，原 39 项整数采样和两项拒绝仍明确绑定初版二进制，不将它们冒记为最终二进制重新执行

最终 game SHA-256 为 `457f7d22d1ae94ef9801f16e2507ac2a50d17d950ff44112af9e715cbec07091`，lab 为 `f1e1e93d0fa22bdb96159cb935c603bfab4c471f5b76416c64b3f2f80dfe0a5e`，150 个构建输入前后相同；命令、日志与源码身份在 `target/stage-v2-runtime-20261003/validation.json`，SHA-256 `b41967b0ff2e5ec4dc072a38a774d1dde114b228a5d7c507a7e336878955b23c`。首次测试编译的 Children 迭代及 i64 推断失败也保留；本批没有新增第三方版本、图像或音频素材

上述结果证明这批软件与静态场景行为，尚未验收真实手柄 / 音频、Windows / ARM 运行、设备帧时间、真人动晕 / 音乐预期、自动音乐编排或带版本的视觉 Replay；完整项目与 08 的退出继续保留这些条件

舞台提交 `81d2849` 已推送至 main，[Lightweight CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37117475442) 通过 62 项测试并恢复缓存；150 项冻结构建输入与该提交逐项一致，交付记录 `target/stage-v2-runtime-20261003/delivery.json` 的 SHA-256 为 `80591959d1b6a6e82bfd86b186f31357d5554b2de2f65ed56446b14b741e05ea`，CI 不包含 runtime / GPU 验证

## MIR 时间背景门控的退化记录

2026-10-03，在分母下限候选已有峰上增加固定 `E[i] >= mean(E[clipped i-8..i+8]) + 0.5` 门控，均值包含当前窗与零值，边界只使用实际窗；结果严格为旧峰的子集，不重新选峰或移动坐标。唯一候选的 [运行前声明](../testdata/synthetic/mir-flux-gate-probe/declared-band-background-20261003.json) SHA-256 为 `605f83ed62e7a9eb14e3d2905f7948bfe0dbb5b21f7d9bbf85c7a034a632ee4c`，源码 SHA-256 为 `434d3708ede7f9201a92dac408a6b4eca636538631f5a8544d3067b674387756`，正式运行前后保持不变

8 项软件测试、隔离 fmt / Clippy / release 构建通过；唯一一次 34 项正式运行退出 1，31 项离散为 20 PASS / 11 FAIL，额外峰从 269 降至 4，漏检从 6 增至 16，另 3 项连续控制不评分。noise burst 恢复 PASS，两项 384 帧近邻及弱声部叠加仍 PASS，但两项相位扫描各新增 4 次漏检，snare 漏掉首攻击，stationary-noise 原来误配到首帧的峰被删除，目标修复与生产准入均 FAIL

剩余 3 个 kick 额外峰支持窗跨过构造音符的截断，1 个 snare 额外峰仍处于衰减内；相位脉冲 E 约 0.520 / 0.524，背景门槛约 0.559，明确保留这些真实删峰而不改标签或补点。全部 34 项实际 PCM 的独立 f64 FFT 预测一致，四个旧基线的 51,158 窗及五路 Matcher 全部复现；数值检查脚本曾因 Python 求和策略产生 1 ulp 差异，改为与 Rust 实现一致的逐项左折叠后通过，候选公式与质量门槛未改

执行、日志和全窗证据位于 `target/mir-band-background-20261003/`；实际工具 SHA-256 为 `8f4d14e7d4c40e0f56b9b98d2a7e932744aef0a58db2a39135867bdf4f77ebd3`，报告为 `a74c95773476534c45740ff39a49cde13e1e81f2301845e36e1391bc3742da28`，[持久观察清单](../testdata/synthetic/mir-flux-gate-probe/observations-band-background-20261003.json) 为 `91c66d532f697b96a3a533d43ba09af5cb44c997f3e1e5cb98b0de62a8003228`。未新增 PCM、依赖或生产 MIR 接线，真实音乐、编码回读、人工标签和音乐置信度仍待独立验收

该批以 `7e7bd66` 提交并推送，[Lightweight CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37119012035) 通过 62 项测试；远端提交与八项文件身份已核对，交付记录 `target/mir-band-background-20261003/delivery.json` 的 SHA-256 为 `d499edfee155dfcf28cd3c8808dac9b8187b92bdd5b7b95274ec472221cfc717`，CI 软件通过不改变上述候选质量 FAIL

## Anchor 提案审阅与明确采用

2026-10-03，media 新增纯 AnchorProposal 编译器，lab 接入 `propose-anchors` / `adopt-anchor-proposal`：显式策略、原始整数帧、稳定选择、全部接受 / 拒绝证据、完整来源与报告重编核对，再通过原保真导出替换明确选中的 Anchor，详细契约见 [Anchor 提案](anchors.md)

`cargo test --locked --offline -p cocobeat-media -p cocobeat-lab` 通过 41 项测试，其中 media 32、lab 9；新增的 4 项纯编译测试和 3 项报告 / 采用测试覆盖两侧冲突、确定性、未知置信度、100,000 项边界、f32 正负零、报告篡改、选择错误与字节上限。workspace Clippy、格式、边界和 game / lab 构建通过，没有新增依赖或修改 SongPackage / Replay 格式

真实 64 秒开发音频保持原字节，通过现有 `build_package` 构建 15 条明确声明的测试 onset，保留原真实能量、分析段落与 chart；置信度使用精确 Q8 分数，只验证选择机制，不代表音乐标签。独立整数 oracle 用完整已选集合检查冲突，主策略 `0.5 / 480000` 选择原索引 `[3,7,10,12,13]`，即 10 / 20 / 30 / 50 / 60 秒，全部证据逐项吻合，重复报告字节相同；原手工包的空 onset 始终返回空提案

冻结程序执行 32 项 CPU 检查，含 6 次提案、4 次实际采用、16 次拒绝、3 次直接 core / Replay 对照和 3 次生产 runtime Replay。完整、乱序子集和显式清空均经独立 Postcard / BLAKE3 读回，原音频 / analysis 字节、cue / rules 保留，新身份正确；原两个包与所有已创建输入维持快照，失败不改已有目标或留下新包

采用后的 5 / 3 / 0 个 Anchor 分别得到 16 / 10 / 1 个规则事件，逐项核对原 ID、玩家、输入序号、Precise 零偏差、AnchorSync 内嵌判定和 40 秒 FreeSync；直接 DuoEngine 与 Replay 重放的事件、Resonance 完全一致，编解码往返一致。实际游戏接受新包及匹配 Replay，拒绝旧身份 Replay，原包仍接受旧录制

独立复核补齐消费 helper 的 AnchorSync 内嵌判定比较后才执行矩阵；原超长选择负例还包含缺字段，故保留原日志并只补一次合法 JSON 加空白至 1 MiB + 1，明确得到字节上限错误，没有重跑整套矩阵或修改生产实现

152 项构建输入前后一致，最终 lab SHA-256 为 `873014eb76f3edaefaa13c8d5145e70282416c985456e8bdf133a6c726f3feb0`，game 为 `8672f50f6168c8f89b3751f656ef8c6b561465e153e960b9bdbacf4c5c629219`；原始命令、源快照、独立 oracle 和四个采用包的身份记录于 `target/anchor-runtime-20261003/qa-report.json`。本批没有新 UI 或 GPU 验收，真人试听、置信度校准、独立音乐标注、可玩性、Windows / ARM 和物理设备继续单独验收

该批以 `f19a069` 提交并推送，[Lightweight CI](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37119902197) 通过 66 项测试，152 项构建输入与该提交吻合；174 项 QA 证据封存清单 SHA-256 为 `c5c3f35b8ded8de6dd7972714e755a395ad20ae1d14206892a44a838d08086d8`，交付记录 `target/anchor-runtime-20261003/delivery.json` 为 `56267d64bac21278a85272c0035aec7120bfb8f35aaa9e1ef13793fa8e44f0bf`

## Replay JSONL 事实与规则诊断

2026-10-03，`inspect-replay` 完整验证真实歌曲包和 Replay v1，复用唯一 core 重放入口，将原始输入 / 水位、已确认事件及整数 Hit 关联逐行输出；未知或不足水位不补 Miss，报告没有设备时间或推算的输入延迟。core 语义错误保留原事实序号，Replay 格式与成功规则结果保持，完整契约见 [Replay 诊断](replay-diagnostics.md)

首版 `cargo test --locked --offline -p cocobeat-replay -p cocobeat-lab` 通过 20 项测试，包含 lab 13、replay 7 和十分钟输入历史重放；workspace Clippy、格式、边界及 game / lab 构建通过。独立复核指出测试不能把缺失 nullable 字段当成 null，也不能让空事件列表绕过 Miss 检查；随后只补强现有测试的完整对象 / 数量，以及交错水位后的原事实索引，4 项 lab replay 定向补验、Clippy、格式和构建再通过

补强只涉及测试，最终两个二进制与首版字节相同，最终 153 项构建输入前后一致。最终 lab SHA-256 为 `ea4c472be808eb1cc19b2a08f28f94ce938aa7011995181fae3eb77c3b55e4ba`，game 为 `904eb1aaf1b4ed5dacae33d17ed8e93ae45c0c35c4a2f46b1ac8c8e83e0a1a2d`；Cargo.lock 仅增加 lab 到已有 core / replay 的两条本地依赖，没有新增第三方版本

最终程序完成 20 项真实 CPU 检查：10 份成功 JSONL、8 次明确拒绝和 2 次 runtime 对照。报告逐行与完整显式整数对象及类型比对，包含三类事件、四等级、非零偏差、奇数中点、`u64::MAX` / 大于 2^53 的 seq、插入水位后的原事实索引；空和单方水位仍无已确认事件，部分历史只确认首个 Anchor 并保留其余 4 个 pending

原包、无变化导出和全撤销导出生成完全相同报告，重复诊断字节相同；真实改谱在 lab / runtime 都拒绝旧身份、接受新身份。负 Hit、EOF Hit、重复输入、版本和目标路径错误均返回对应原因，已有目标、原包和录制字节保持；源包内部输出和源 Replay 同路径负例使用新目录中的等字节副本，既有封存目录始终只读

所有命令、预期对象、实际 JSONL 和输入快照记录于 `target/replay-diagnostic-runtime-20261003/qa-report.json`，原始完整检查与测试补强后的检查分别保存在根目录及 `final/`；没有产品失败或矩阵补跑，也未执行新 GPU、设备延迟、真实控制器、Windows / ARM、真人体验或图形时间线验收

## 原创曲目的标注准备

2026-10-03，进一步只读核对 MIR 失败：真实 snare 首击的 `E-B` 小于剩余假峰，单一阈值无法同时保留该真峰并排除这些假峰，停止当前门控修补链；诊断位于 `target/mir-next-background-20261003/diagnosis.md`，SHA-256 `ed6761214d78e74541fec012ed599ef2ae1c0604f8c6f04f1d131e737a3e8dd2`，未运行新候选或修改旧结果

[审阅清单](../testdata/synthetic/dev-song-review/README.md)仅从既有原创 64 秒 WAV 及已核对配方生成 438 个声部起点 / 200 条合并来源候选，另含 7 个结构边界和原 7 个创作 Anchor。人工 onset 帧 / 不确定区间 / 审阅者及可玩性字段全部 pending / null；静音内部没有候选，独占 EOF 单列，反相 hat 的立体声与下混边界有明确说明

生成器使用 Python 标准库，实际 WAV、Rust 配方和两个 CSV 的身份均校验；逐字节复现、防覆盖后字节保持、来源漂移拒绝及全部计数 / 人工字段检查通过，独立复审为 `Lean already. Ship.`。验证记录在 `target/dev-song-review-20261003/validation.json`；`review.json` SHA-256 为 `423cb400130a28bb91e46de29eea9124a9656a5d8ddc3c066a24dbe4a64e8404`，来源与许可已登记，没有新音频、外部语料、MIR 运行或人工听感验收


## 受邀请的 QUIC 可靠历史软件会话

2026-10-03 的 `net-host` / `net-join` 使用最新稳定 Quinn 0.11.12、quinn-proto 0.11.19、rustls 0.23.45、rcgen 0.14.10 与已有 Tokio 1.53.1，manifest 保持主版本范围；新增 39 个第三方版本，完整台账为 586 项，既有版本未移除或替换，runtime / game 尚未接入网络

`cargo test --locked --offline -p cocobeat-net -p xtask` 的 11 项测试通过；首轮 Clippy 暴露测试对 `usize::MAX` 临时值取可变引用，改为局部变量后重跑 7 项 net 测试及相关 all-targets Clippy 通过，原失败日志保留。net 的依赖白名单进一步明确拒绝 Bevy / Kira，4 项 xtask 测试、Clippy、格式和边界另行通过；测试重复执行不重复计数

lab 实际构建通过，冻结二进制 SHA-256 为 `899447d5777dc198443d1eeaf6970db38307390de835b439df109515fbd58836`，受控 peer 为 `9ce1d80ffe7878eb1900d91a9e30958ad7a86358d27f282364c9386ca63aed4f`；196 项源码输入与命令记录在 `target/net-runtime-20261003/`，lab 构建期间只有不在其依赖链内的 xtask 白名单修改，已在修改后独立检查，工作台改动在 lab 冻结后才接入，不混入本批网络证据

13 个场景 / 30 条真实进程命令通过：两个产品 OS 进程在实际 loopback UDP / QUIC 连接中完成邀请、Ready / Start、可靠历史和 FinishAck；135 条事实的 P1/P2 子序列分别为 68 / 67 项，得到精确 16 个规则事件，双方权威 Replay 字节相同，实时 core 与保存后的 Replay 重放一致。受控 guest 逐条发送及延迟最终水位后得到相同完整结果，所有诊断 JSONL 逐字段与独立字面规则预期比较

终态水位前断线实际保留 68 / 66 项历史，双方 Hit 已到而 P2 仍停在 -1 水位，0 个事件、7 个 Anchor 待确认，不补 Miss；认证后的超终点水位拒绝且不录入错误事实。错误 FinishAck 哈希和只 FIN 未发应用 Ack 都保持 `AUTHORITY_VERIFIED_UNCONFIRMED`，不会报告 COMPLETE

不同合法包和错误 token 在真实 TLS 后的 Hello 阶段拒绝，合法替代 DER 配自洽指纹在实际 TLS 握手拒绝；单改指纹仅算本地邀请校验，缺尾水位、已有输出和源包内输出仅算本地预检，二者均与真实连接案例分开记录。源包、模板、验证脚本及既有输出保持原字节，全部自有进程正常收敛；没有输出 token 或私钥

逐进程耗时含本地包验证、连接和关闭，135 项事实不证明最大容量性能；独立既有 144,004-fact Replay 压力测试也不等于 QUIC 的容量验证。本批没有 GPU、音频、物理输入、两台机器或 LAN/WAN 结果，同信任根下另一 leaf 的独立 pin 分支仅有代码审查；资源传输、ClockSync、未来 ScheduleStart 和正式游戏网络入口继续在 todo/10 推进

## 原生波形与 Anchor 工作台

lab 的 `workbench PACKAGE NEW_PACKAGE [--locale CODE]` 接入独立静音 Bevy 窗口，复用已验证 PCM、AnchorEditor、保真导出及现有字体 / 语言 / 菜单主控判定；键鼠编辑、手柄浏览、精确整数帧、虚拟列表、Undo / Redo 和失败保留契约见 [内容编辑](editor.md#原生工作台)

2026-10-03 历史批次曾完成 35 项相关测试及五种实际窗口流程，覆盖精确改帧、撤销恢复、密集列表、EOF 拒绝、源身份变化与目标冲突后的失败保留；发现并修复图像与指针边框错位、详情横向裁切和滚动重置。滚动问题来自 Bevy 为所有 Node 自动附带 ScrollPosition，旧查询遍历按钮等无溢出节点反复将共享偏移限为零，修正为只更新详情节点，并留下实际 setup / update 的三档 DPI 回归。该批 `target/workbench-runtime-20261003/` 原始日志和截图在本次恢复环境中已不存在，历史结果与本轮复查分开记录

2026-10-05 重新验证的代码与暂停时工作台三文件 SHA-256 一致，完成 9 项工作台测试、20 项 runtime 输入测试、3 项 i18n 测试和 3 项设置测试，共 35 项不同测试；runtime / lab all-targets Clippy、格式、依赖边界及 game / lab 构建全部通过。测试包含波形跨块与尾帧、拖动单次历史、最大 ID、整数编辑、混合主控、双手柄事件、认领吞键、失焦断连释放屏障、DPI 命中、实际图像边框和详情滚动隔离；Cargo.lock 仅增加 lab 对已存在 Bevy 的直接依赖，没有新增第三方版本

本次额外执行 10 条实际 CLI 命令，验证合法源包、错误 locale / 参数 / 源路径、已有输出和源目录别名拒绝，以及实际改帧 / 全撤销导出后重新验证；源四对象保持原字节，改帧保留 audio / analysis 字节并更新 chart / manifest，全部撤销的四对象与源一致。这验证现有导出路径和工作台启动拒绝条件，本次未重跑历史 GUI 导出及失败恢复流程

两场独立 Gamescope headless XWayland 使用本机已有 RADV RX 6650 XT，原生 Bevy Screenshot 读取 GPU 图像，1280×800 与 640×480 共 8 张图逐张目检通过。仅 target 中的辅助程序增加初始尺寸、软件 KeyboardInput 驱动和截图系统，生产 input.rs / ui.rs 保持原样并通过真实捕获系统消费这些消息；64 秒原创包含 7 个 Anchor、48,000 组立体声峰值和混合 CJK 长 SectionCue。640 详情的状态、Computed ScrollPosition 和原生画面一致，按键从 0 移动至 48，再到实际底部 263 px，底部手柄帮助完整可读；1280 全部详情容纳时偏移保持零。两场均保持未修改草稿且没有创建导出目标

沙箱内未暴露 GPU，首个无头探针报 `failed to find physical device`，通过已授权的独立 GPU 验证环境完成补验；没有操作用户桌面或安装系统软件。辅助程序是 target 内的独立二进制，这些软件按键和原生读回不代表正式程序的物理键鼠 / 手柄验收；CJK 分词诊断仍按既有上游限制保留

本次 160 项已列出的 Rust / Cargo / 配置 / 语言 / 字体 / 旗帜输入前后哈希一致，实际命令、构建身份、源码哈希、CLI 结果及截图 / 状态摘要已保存为可提交的 [观察记录](../testdata/synthetic/workbench-observations-20261005.json)，原始产物在 `target/workbench-milestone-20261005/`。本批不证明物理呈现性能、Windows / ARM 窗口、音频试听、真实双手柄、13 种语言的真人翻译质量或完整编辑器退出；候选证据、Replay 图形诊断与设备计时关联继续留在 todo/09

## QUIC 四对象资源接收

2026-10-07 在 `7fe4016` 后的资源接收源码冻结版本验证，新增 `net-receive`，protocol / ALPN v2；记录见 [持久化观察](../testdata/synthetic/quic-resource-observations-20261007.json)，原始日志与 helper 保存在 `target/quic-resource-20261007/`，后者可随构建缓存清理，完整阶段退出继续按 todo 10 / 11 / 12

`cargo test --locked --offline -p cocobeat-media package::tests` 的 17 项、`cargo test --locked --offline -p cocobeat-net` 的 8 项通过；`cargo clippy --locked --offline -p cocobeat-media -p cocobeat-net -p cocobeat-lab --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`cargo xtask boundaries` 和 lab 实际构建通过。首次 net 编译的两处 Rust 类型推断错误已修复，失败日志保留，当前结论来自后续通过的冻结代码

`python3 tools/quic-session-check/check.py target/debug/cocobeat-lab target/quic-resource-20261007/loopback-host` 实际运行 16 条命令，覆盖未预装接收、预装加入、超过 64 条的批次、原始四对象保真、两端相同权威 Replay、core JSONL 诊断、模板错配、已有文件 / 包 / 输出 / 符号链接保护及旧协议拒绝；有效包发布后模板错误保留有效包，事实保持 `[0,0]` 且会话 FAILED

恶意 host helper 通过准确 Cargo JSON 的 10 个现有 rlib 用 rustc 构建，在私有 loopback 与实际生产 lab 接收程序通信，6 类输入分别为超长 descriptor、坏对象 hash、截断、尾随数据、错误预期包哈希及额外 uni stream；每端均 exit 1 / FAILED、事实 `[0,0]`、未观察到 Installed / Ready、无发布目录和 staging 泄漏。helper 复制冻结 net 实现后附加测试入口，原始源码、构建参数和摘要 hash 留在本批 target 记录，不替代生产游戏接线验收

sandbox 首次绑定 UDP 返回 `Operation not permitted`，记录保留；获批后在实际主机只使用 loopback 重跑成功，未连接远端、操作设备或修改系统配置。最大容量性能、进展超时实际到期、真实双机、游戏窗口联网、音频设备同步和真人体验为 NOT RUN；异步期限不能抢占同步校验，已有本地空目录发布竞态边界见网络会话文档

## QUIC 网络时钟与预约软件起点

2026-10-07 在资源接收提交 `544afb4` 后的冻结源码验证 protocol / ALPN v3；[持久化观察](../testdata/synthetic/quic-clock-observations-20261007.json) 保存源码 / 二进制 / 日志摘要、实际状态与 QA 注入差异，原始记录位于 `target/quic-clock-20261007/`，可随构建缓存清理。net 的 13 项单元测试、lab 实际构建、net / lab 全目标 Clippy、格式与依赖边界通过；原媒体接收代码未修改，上一里程碑证据独立保留

`python3 tools/quic-session-check/check.py target/debug/cocobeat-lab target/quic-clock-20261007/loopback` 的 16 条实际命令通过，接收 / 预装两组均完成真实 QUIC 与相同权威 Replay，并核对同一个 host 起点、软件唤醒不早于本端预约、迟到量等于两者差值且不超过 100ms，资源、错误模板及目标路径检查继续通过

QA helper 由准确 Cargo JSON 的 10 个既有 rlib 构建，复制冻结 net 六源文件，只在副本 sync.rs 的四个发送触点注入故障，完整 host / join 协议及实际生产 lab 另一端保持原路径；5 组实际 loopback 分别为首次 reply 丢失并夹错 epoch / id / send timestamp / 旧重复回复、全部八次 reply 丢失、错误 ClockSynced transcript、过晚 ScheduleStart 和缺 ScheduleStartAck。恢复组 production guest 两次探测、至少四个忽略包后双方 COMPLETE，权威 Replay 原字节相同；其余两端均 FAILED、事实 `[0,0]`、无权威文件及软件开始观测，错误原因匹配对应门控，QA trace 证明注入触点实际执行

升级后另外重跑 6 类恶意资源传输均通过，坏 descriptor / hash、截断、尾随数据、错误包身份和额外 uni stream 仍无法发布或进入 Installed / Ready。三组 suite 使用生产二进制，QA host / peer 的复制和注入边界见持久化观察，不将该 helper 当作未经修改的另一端生产二进制

默认 1000ppm 相对单调时钟漂移是模型假设，host start uncertainty 为 0 只代表坐标恒等，网络不对称 / 样本老化 / 未来起点误差另有区间；软件唤醒迟到另记，不能当作 Audio ClockBridge、Kira callback、设备或声学同步精度。真实双机、实测漂移假设、游戏窗口实时输入 / 音频接线、超过 100ms 的调度卡顿故障和最大 datagram flood 为 NOT RUN，完整阶段退出继续保留
