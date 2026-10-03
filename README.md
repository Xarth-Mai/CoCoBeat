# CoCoBeat

<img src="assets/brand/icons/symbol-256.png" width="128" height="128" alt="CoCoBeat 静态图标">

> Co. Co. Players, One Beat.

雨夜霓虹里的双人节奏游戏：机器提供稀疏、可信的音乐 Anchor，两位玩家用真实的输入填满其间的自由空间；先证明两个人会倾听、模仿和共同落点，再扩展音乐导入、自动分析与联网

## 当前状态：本地双人原型，待真实设备验收

已实现 Bevy 3D 场景、Kira 播放、原创 64 秒开发音乐与 7 个手写 Anchor、键盘/手柄菜单、Free Sync、Anchor Sync、Resonance 和本地 Replay；游戏可选择内置开发歌曲或通过 `--package` 播放已验证的手工内容包，实验工具已接入有资源上限的源音频解码、48 kHz 重采样、严格 canonical 读回和手工内容包事务，完整歌曲导入、自动 MIR、编辑器与联网仍在后续路线图

早期完整软件基线通过 97 项测试，16 组软件计时情景、Replay CLI、原生 Logo 停靠、Ready 眼睛循环与 13 个语言变体的 GPU 离屏界面均已有验证；画质与帧率设置里程碑的软件检查及 46 张 GPU 截图均为 PASS，覆盖低/中/高/关闭效果共 4 张画质场景、39 张设置页面与 3 张语言列表；小窗口/DPI 设置已有 25 张截图通过；极小 Ready 菜单的越界和遮挡已修复，该批 28 张菜单与设置截图逐张检查通过；真实窗口、呈现 FPS、VSync、物理输入、音频延迟、听感和真人双人体验均为 NOT RUN，具体证据见 [验证策略](docs/testing.md)

```text
apps/cocobeat-game       组合入口
crates/cocobeat-schema   整数时间、输入身份与规则事件
crates/cocobeat-core     Anchor 判定、一对一配对与有界 Resonance
crates/cocobeat-runtime  Bevy / Kira、ClockBridge、输入、会话与表现
crates/cocobeat-replay   有界 JSON 持久化与同一 core 重放
crates/cocobeat-media    有界音频处理、严格读回与四对象内容包事务
tools/cocobeat-lab       研究实验，不进入正式游戏 UX
xtask                   开发检查命令
assets/dev              开发资源约定
testdata                测试数据与来源约定
docs                    产品与技术约束
todo                    有验收条件的路线图
licenses                依赖与资源来源台账
```

## 开始开发

安装 [rustup](https://rustup.rs/) 和本机图形/音频/手柄开发依赖后，在仓库根目录运行；平台要求见 [构建说明](docs/build-release.md)

```sh
rustup update stable
rustup component add rustfmt clippy
cargo xtask doctor
cargo xtask check
cargo run --locked -p cocobeat-game
```

Rust 跟随最新 stable，Edition 2024。依赖采用最新稳定版本，Cargo manifest 使用主版本范围（如 `"1"`），GitHub Actions 使用最新稳定主版本标签（如 `@v7`）。`Cargo.lock` 提交到仓库并固定实际解析版本，常规构建使用 `--locked`；升级时运行 `cargo update` 并重跑检查，跨主版本时更新 manifest 和适配 API。研究报告中的版本号只作为历史参考

正常启动完整播放原生 Logo 动画，再将同一 Logo 移至左上角并显露界面，随后保持 Ready，主菜单播放眼睛循环；开始歌曲、暂停和结算时使用静态定稿，失焦冻结菜单动画，恢复后继续；音乐在用户另行选择 Start 后播放，片头期间的按键和手柄操作不会穿透到游戏，窗口关闭仍有效；音频输出初始化失败会明确退出

| 操作 | 键盘 | 手柄 |
|---|---|---|
| 菜单选择 / 确认 | 上下方向键 / Enter | 十字键或左摇杆 / South（下侧键） |
| 副控接管菜单 | Enter，首次只接管 | Start，首次只接管 |
| P1 / P2 Hit | F / J，可在“双人输入”页重绑定 | 在“双人输入”页明确加入 P1/P2，默认 South，可重绑定 |
| 子页返回 | Esc | East（右侧键） |
| 暂停 / 恢复 | Esc | Start |
| 重新开始 | F5 或菜单 | 菜单 |
| 保存 Replay | F6 或菜单 | Select 或菜单 |
| 返回主菜单 | 菜单 Main menu | 菜单 Main menu |
| 设置 | 主菜单 / 暂停菜单 Settings | 主菜单 / 暂停菜单 Settings |

显示设置提供分辨率选择、无边框全屏、应用、取消和恢复默认；尺寸变更先预览 15 秒，窗口实际状态确认后才可保存，取消或超时恢复；普通窗口与全屏渲染尺寸分别保存。配置位于 Windows 的 `%APPDATA%/CoCoBeat/settings.json` 或 Linux 的 `$XDG_CONFIG_HOME/cocobeat/settings.json`（未设置时使用 `$HOME/.config/cocobeat/settings.json`），读取或保存失败会显示提示；实际 WM、DPI 和跨屏行为尚待平台验收

画质设置提供低、中、高预设，默认中档；可独立调整 MSAA、雨量、雾、阴影与 Bloom，单独调整后标记为 Custom。帧率上限按当前显示器报告的最高刷新率生成每 60 一档并追加最高档，同时提供无限制；默认最高档，未知刷新率时使用 60，VSync 独立设置且默认关闭，实际呈现帧率与 VSync 效果尚待设备测量

画质、帧率、语言与显示设置共用草稿，仅保存成功后提交；显示预览取消或超时恢复整份设置，子页返回保留草稿，关闭设置不会续播歌曲。旧 v1 配置补入默认画质与帧率设置；手改配置中预设标签与独立项不一致时保留各项并标记 Custom

语言设置支持 `zh-CN`、`en-US`、`en-GB`、`ja`、`ko`、`zh-TW`、`zh-HK`、`es-419`、`pt-BR`、`fr`、`de`、`ru`、`uk` 共 13 个变体，英文分别提供美国与英国版本，繁体中文分别提供台湾与香港版本；首次启动跟随系统语言，未支持时使用 `en-US`，应用后立即更新界面并跨启动保存，与显示设置共用草稿、取消和预览回退流程

菜单、HUD、设置与玩家可见提示统一通过翻译表呈现，文字采用 Noto Sans，中文、日文与韩文使用对应地区字形；语言选项显示本名与地区旗帜，SVG 来自开源 flag-icons，运行时使用同源 PNG，来源见 [字体](assets/fonts/README.md) 与 [旗帜](assets/flags/README.md)。国际化里程碑的 29 张离屏截图已检查字形与换行，母语真人校对仍为 NOT RUN；当前稳定引擎的 ICU CJK 词边界诊断及后续处理见 [验证记录](docs/testing.md#国际化与字体里程碑)

支持双人键盘、双手柄及键盘＋手柄，P1/P2 的 Hit 与按住状态独立；菜单同一时刻由一个设备主控，主控身份与 P1/P2 分开显示。首次明确按键只取得控制权，副控方向和普通确认不抢焦点，Enter/Start 可接管但不会顺带执行选项；实际暂停的设备接手菜单，主控断线后保持暂停并等待新设备接管

已属于同伴的手柄不能静默转移给另一位玩家；左摇杆每次推过阈值移动一步，归中后可再次移动。设备断连后需从“双人输入”页重新加入，绑定当前只保留在本次运行中；窗口失焦会暂停，恢复后释放已按住按钮再继续，详见 [输入与主控规则](docs/platform-input.md)

结束歌曲、重新开始、返回主菜单、正常关闭窗口或手动保存时，输入历史写入当前工作目录的 `replays/`，同名 CSV 保留输入观察/消费时间与映射不确定性；返回主菜单会停止歌曲并重置会话，保留输入绑定，需释放确认键再重新按下才能开始歌曲；异常终止不保证保存未落盘历史

```sh
cargo run --locked -p cocobeat-game -- --package path/to/song-package
cargo run --locked -p cocobeat-game -- --package path/to/song-package --replay path/to/session.json
cargo run --locked -p cocobeat-game -- --package path/to/song-package --visual-smoke target/package-preview.png
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
cargo run --locked -p cocobeat-game -- --viewport-smoke languages en-GB 1280 800 2 2 target/cocobeat-dpi.png
cargo run --locked -p cocobeat-lab -- timing-sim
cargo run --locked -p cocobeat-lab -- generate-dev
cargo run --locked -p cocobeat-lab -- decode-audio input.wav new-output.f32le
cargo run --locked -p cocobeat-lab -- resample-audio input.wav new-output-48k.f32le
cargo run --locked -p cocobeat-lab -- readback-canonical testdata/synthetic/media-import/stereo-canonical.ogg 4800 new-canonical.f32le
cargo run --locked -p cocobeat-lab -- prepare-audio testdata/synthetic/media-import/stereo-canonical.ogg 4800 new-audio-staging
```

`--package DIR --replay FILE` 用完整包身份、包内 Anchor 和实际歌曲时间范围校验历史，相同音频但不同谱面的包也会被拒绝；`--package DIR --visual-smoke PNG` 在整数帧中点渲染当前包的时长和下一个 Anchor。单独 `--replay` 用相同 core 校验开发歌曲历史；`--visual-smoke` 只渲染预设场景并保存 PNG，不播放音频或运行玩法；`--startup-smoke` 完整运行品牌时间线，使用生产菜单控制系统开启 Ready 眼睛循环，在循环 6.1 秒时保存界面，不播放音频；`--settings-smoke` 使用模拟的 1280×800 显示表面和 640×480 场景验证 letterbox 与原生分辨率设置文字，不读写用户配置，也不证明真实窗口模式转换；lab 默认把模拟报告和 WAV 分别写入 `target/timing-sim/` 与 `target/dev-assets/`，均支持目录参数，详见 [计时说明](docs/timing.md) 与 [开发内容](assets/dev/vertical_slice/README.md)

`--locale-smoke CODE PNG`、`--menu-smoke CODE PNG` 和 `--language-smoke CODE PNG` 分别保存指定语言的设置、Ready 主菜单和语言选择页，`CODE` 使用上述完整语言代码；三者运行品牌呈现到 Ready，并使用固定时间步加速离屏预览，不播放音频、不读写用户配置，不代替原生窗口或物理输入验收

`decode-audio` 支持 WAV/PCM、FLAC、MP3 和 Ogg Vorbis，输出原采样率的立体声 F32LE，单声道复制为双声道；保留静默与原始幅度，源文件限 512 MiB、192 kHz、十分钟。`resample-audio` 复用该入口，以 OxiMedia High 转为 48 kHz，48 kHz 原件直接保留样本；实际输出帧数为 `ceil(源帧数 × 48000 / 源采样率)`，不裁静默或归一化

`readback-canonical <input.ogg> <expected-frames> <new-output.f32le>` 完整读回最终文件，严格要求 Ogg Vorbis、48 kHz、恰好双声道、有限样本和从帧 0 开始的连续时间轴；同时检查 Ogg 页 CRC、EOS 与实际帧数，`expected-frames` 来自编码器输入帧数，不能直接取待验证文件的时长声明。上面的 [原创样本](testdata/synthetic/media-import/README.md) 为 4,800 帧；三个命令均只创建新输出并在失败时清理半成品，生产编码器准入与游戏中的完整导入流程仍待完成

`prepare-audio <final.ogg> <expected-frames> <new-staging-dir>` 将最终 Ogg 有界复制到全新的目录，按实际复制字节记录 BLAKE3 和长度，关闭写入后严格读回其中的 `song.audio.ogg`；原文件保留，已有目录拒绝覆盖。失败只清理本次音频文件和空目录，成功只表示该音频对象已准备，不创建分析、谱面、manifest 或 Ready；源上限仍为 512 MiB 和十分钟

`build-authored-package <final.ogg> <expected-frames> <authoring.json> <new-package-dir>` 从自有的最终音频副本计算能量，组合手工 Anchor 和段落，校验全部对象后原子发布新目录；`verify-package <package-dir>` 完整复核已有包。格式、版本、限额及 64 秒开发歌曲的创作样例见 [SongPackage](docs/song-package.md)，`--package DIR` 会校验完整包并播放其中的音频，使用实际长度和手工 Anchor；同一包支持上述 Replay 校验和离屏预览。无参数仍使用内置开发内容，自动 MIR、AnchorCompiler 与舞台编译继续按路线图推进

`--quality-smoke low|medium|high|off PNG` 保存指定画质的固定场景，`off` 关闭 MSAA、雨、雾、阴影与 Bloom；`--settings-page-smoke graphics|pacing CODE PNG` 通过生产菜单控制进入画质或帧率子页，运行品牌到 Ready 后截图。两种预览均不播放音频、不读写用户配置，也不运行生产帧率门控，不能用截图耗时推断实际帧率

Ready、暂停、结束、故障和设置菜单共用实测行高滚动，操作项、所有 13 个语言选项及说明/错误都可聚焦，超长行用上下键分段阅读；Windows 读取显示器工作区，X11 使用桌面工作区与当前显示器的保守交集，Wayland 未提供的信息保持未知。`--viewport-smoke main|graphics|pacing|languages|ready|paused|finished|fault CODE WIDTH HEIGHT SCALE ROW PNG` 可检查指定物理尺寸及缩放，ROW 从 0 开始；软件修复及尚未执行的实际桌面验收见 [本轮验证](docs/testing.md#小窗口游戏菜单)

准备真实音频采集时，可显式运行 `cargo run --locked -p cocobeat-lab -- audio-probe 30 target/audio-probe-30` 播放点击音并记录软件游标；它不捕获 loopback，也不测量物理输出延迟，支持的时长与结果说明见 [计时说明](docs/timing.md)

## 平台、输入与发行构建

初版交付范围为 **Windows / Linux × x86-64 / ARM64**，以及双手柄、键盘＋手柄、双人键盘；输入流程已有软件实现，各平台真实设备兼容性仍需分别验收

```sh
cargo build --locked --release -p cocobeat-game
```

发行配置使用 `opt-level=3`、fat LTO、单 codegen unit 和 `panic=abort`，关闭调试信息与增量编译，保持通用 CPU 基线；优化收益仍需在真实游戏负载上测量

GitHub 自动 CI 检查单 Linux 下的格式、依赖边界和纯逻辑；Windows/Linux 各 x86-64 / ARM64 发行构建从 **Actions → Manual release build → Run workflow** 手动选择，成功后下载对应 Artifacts，流程与验收范围见 [构建与 CI](docs/build-release.md) 和 [平台与输入](docs/platform-input.md)

Windows EXE 内嵌多尺寸静态图标，Linux 包使用 `bin/` 与标准 `share/applications`、`share/icons/hicolor` 布局；Linux 用户级安装见 [说明](packaging/linux/README.md)，实际桌面显示与四目标发行验收仍需分别执行

## 不变的边界

- schema 定义事实；core 定义规则，两者不认识引擎、音频后端或网络传输
- Replay 和未来网络输入经过同一个 DuoEngine
- 未来 AnchorCompiler 消费 MusicAnalysis，输出自有内容，不认识 Bevy
- 判定输出语义事件，表现只能表达事实，不能修改判定
- 自由输入不按隐藏谱面评分；沉默不是 Miss，也不是 Sync
- 每项职责只有一条正式实现路径，不加运行时隐式 fallback

软件时钟不确定性与共享确认参数仍是待测假设，见 [时间契约](docs/timing.md) 与 [规则](docs/gameplay.md)；后续工作见 [文档索引](docs/README.md)、[架构](docs/architecture.md) 和 [工作进度](todo/progress.md)

## 许可证

项目代码使用 **MPL-2.0**，见 [LICENSE](LICENSE)；资源、音乐、测试数据和第三方依赖分别记录来源与许可，见 [许可证台账](licenses/THIRD_PARTY.md)
