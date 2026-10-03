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
