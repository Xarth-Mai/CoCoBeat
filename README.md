# CoCoBeat

> Co. Co. Players, One Beat.

雨夜霓虹里的双人节奏游戏：机器提供稀疏、可信的音乐 Anchor，两位玩家用真实的输入填满其间的自由空间。先证明两个人会倾听、模仿和共同落点，再扩展音乐导入、自动分析与联网。

## 当前状态：Day 0

已建立 Rust workspace、48 kHz 整数时间契约、依赖边界检查、轻量自动 CI 和 Windows/Linux 双架构手动构建。
`cocobeat-game` 目前只打印启动信息，**还不是可玩游戏**；core、replay 的 crate 边界已建立，判定、录制与播放尚未实现。没有提前引入 Bevy、Kira、Quinn 或 OxiMedia。

```text
apps/cocobeat-game       组合入口
crates/cocobeat-schema   项目自己的数据契约，当前实现时间类型
crates/cocobeat-core     纯游戏规则的边界
crates/cocobeat-runtime  未来 Bevy / Kira / 输入 / 表现的接入层
crates/cocobeat-replay   同一套规则的录制与重放边界
tools/cocobeat-lab       研究实验，不进入正式游戏 UX
xtask                   开发检查命令
assets/dev              开发资源约定
testdata                测试数据与来源约定
docs                    产品与技术约束
todo                    有验收条件的路线图
licenses                依赖与资源来源台账
```

## 开始开发

安装 [rustup](https://rustup.rs/)，在仓库根目录运行：

```sh
rustup update stable
rustup component add rustfmt clippy
cargo xtask doctor
cargo xtask check
cargo run --locked -p cocobeat-lab -- time-smoke
cargo run --locked -p cocobeat-game
```

Rust 跟随最新 stable，Edition 2024。新增依赖采用当时最新稳定版本，`Cargo.lock` 提交到仓库；常规构建使用 `--locked`。升级时运行 `cargo update` 并重跑检查，跨主版本升级还需要更新 manifest 和适配 API。不要复制研究报告中的历史版本号。

`time-smoke` 只证明 64 秒等于 3,072,000 个标准音频帧，不测量声音输出延迟。
当前无合成音频、`--dev-song` 或硬件计时命令；这些能力按路线图实现后才公开。

## 平台、输入与发行构建

初版要求同时支持 **Windows / Linux**，构建目标为 **x86-64 / ARM64**，以及键盘和手柄：双手柄、键盘＋手柄、双人键盘都需要验收。手柄覆盖玩家加入/分配、Hit、菜单、重绑定和热插拔；当前输入运行层尚未实现，这些是初版必交付功能。

```sh
cargo build --locked --release -p cocobeat-game
```

发行配置使用 `opt-level=3`、fat LTO、单 codegen unit 和 `panic=abort`，关闭调试信息与增量编译；保持通用 CPU 基线。优化收益仍需在真实游戏负载上测量。

GitHub 自动 CI 只在单个 Linux job 中检查格式、依赖边界和纯逻辑测试，不编译图形/音频运行层或制作发行包。手动打开 **Actions → Manual release build → Run workflow**，选择分支及目标：Windows MSVC 或 Linux GNU，各自可选 x86-64 / ARM64；默认 `x86_64-pc-windows-msvc`，不提供 32 位选项。
成功后在该次运行的 Artifacts 下载带提交号的产物。详细约束见 [构建与 CI](docs/build-release.md) 和 [平台与输入](docs/platform-input.md)。

## 不变的边界

- schema 定义事实；core 定义规则，两者不认识引擎、音频后端或网络传输。
- Replay 和网络输入最终经过同一个 DuoEngine。
- AnchorCompiler 消费 MusicAnalysis，输出 AnchorMap，不认识 Bevy。
- Judge / Duo 输出语义事件；FeedbackDirector 只能表达事实，不能修改判定。
- 自由输入不按隐藏谱面评分；沉默不是 Miss，也不是 Sync。
- 每项职责只有一条正式实现路径，不加运行时隐式 fallback。

第一条体验链是：SongTime → 输入捕获 → 手写 64 秒音乐与 Anchor → P1/P2 Hit → Free Sync → Anchor Sync → FeedbackDirector → Replay。

详细说明见 [文档索引](docs/README.md)、[架构](docs/architecture.md) 和 [路线图](todo/README.md)。

## 许可证

项目代码明确使用 **MPL-2.0**，见 [LICENSE](LICENSE)。资源、音乐、测试数据和第三方依赖分别记录来源与许可，不能由代码许可证推定其授权；见 [许可证台账](licenses/THIRD_PARTY.md)。
