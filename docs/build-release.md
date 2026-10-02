# 构建、性能与 CI

## 发行参数

根 Cargo.toml 的 `[profile.release]` 作用于 Windows/Linux 发行构建：

| 参数 | 当前值 | 用途 |
|---|---|---|
| opt-level | 3 | 以运行速度为目标优化 |
| lto | fat | 跨 crate 的整体链接时优化 |
| codegen-units | 1 | 减少代码生成分区，增加整体优化机会 |
| panic | abort | 发生 panic 时退出，不进行栈展开 |
| debug / strip | 0 / debuginfo | 发行产物不保留调试信息 |
| incremental | false | 发行构建不使用增量编译 |

这些参数增加 release 编译时间，但不拖慢日常 debug 开发。panic 不展开意味着不能依赖 panic 时的 Drop 保存 Replay 或清理事务；正常错误使用 Result，重要事实需在正常路径持久化。
不设置 `target-cpu=native` 或构建机专属 AVX 参数，避免在其他玩家机器上出现非法指令。PGO 等进一步优化等到有代表性游戏负载后再引入，不能把编译参数当作已验证的帧率或低延迟保证。

Linux 本机构建：

```sh
cargo build --locked --release -p cocobeat-game
```

Windows MSVC 本机构建：

```sh
rustup target add x86_64-pc-windows-msvc
cargo build --locked --release -p cocobeat-game --target x86_64-pc-windows-msvc
```

Windows 本地需要对应的 Visual Studio C++ Build Tools 与 Windows SDK；Linux 当前已接入 Bevy 的 X11/Wayland、Kira/CPAL 与手柄后端，构建环境需提供 pkg-config、ALSA 和 libudev 开发文件及所选窗口后端依赖；运行环境还需要可用图形驱动、显示会话和音频设备

开发音乐和简单场景由程序生成，正常启动无需下载素材；音频输出初始化失败会明确退出；仅验证图形时使用显式 `--visual-smoke PNG`，它不启用音频或游戏输入

## 轻量自动 CI

`.github/workflows/ci.yml` 在 main 的代码 push 与涉及代码/构建配置的 PR 执行：

1. 一个 Ubuntu job，安装最新 stable，恢复按工具链、manifest/lockfile 与源码区分的缓存。
2. 全 workspace 的 rustfmt 与依赖图检查。
3. schema/core/replay/xtask 的 Clippy 和测试。

无图形/音频运行层编译、无平台矩阵、无自动 release 打包；新提交取消旧检查，单次限制 10 分钟。只改文档或资源不会触发当前自动 CI。完整检查仍用 `cargo xtask check`，新增非 Rust 源码或构建输入时同步更新触发路径。

## 手动发行构建

`.github/workflows/release-build.yml` 仅有 `workflow_dispatch`，不会在每次提交时编译发行包。

打开 **Actions → Manual release build → Run workflow**，选择分支和 `target`：

| 目标 | 原生 runner | 产物 |
|---|---|---|
| `x86_64-pc-windows-msvc`（默认） | windows-latest | x86-64 Windows EXE |
| `aarch64-pc-windows-msvc` | windows-11-arm | ARM64 Windows EXE |
| `x86_64-unknown-linux-gnu` | ubuntu-24.04 | x86-64 Linux tar.gz |
| `aarch64-unknown-linux-gnu` | ubuntu-24.04-arm | ARM64 Linux tar.gz |

四个目标都是 64 位，不提供 32 位选项。一次手动运行只构建选择的目标；另一平台的 job 会跳过，不启动四机矩阵。Linux 固定 Ubuntu 24.04 作为构建基线，实际最低 glibc/运行库要求还需验证。

工作流需先出现在仓库默认分支，GitHub 才会提供手动运行入口。原生 runner 安装目标、执行带 lockfile 的优化构建，然后核对 Windows PE 或 Linux ELF 的架构字段，避免错误标记产物架构。
成功后上传 `cocobeat-<target>-<commit>` artifact，保留 14 天，包含可执行文件、LICENSE、README、Cargo.lock 与 BUILD-INFO（提交、目标、工具链、profile、文件 SHA-256）。Linux 先打包 tar.gz 保留执行权限。失败时不上传产物，不自动发布 GitHub Release。

目前入口为本地 64 秒双人原型，包含程序生成的音乐与场景；第三方 notices、安装包、运行库与完整资源打包仍属于 V1 加固门槛，手动构建流程和四目标首次结果须按 [工作进度](../todo/progress.md) 验证，不能用本机编译推断其他平台或真实设备兼容性

## 本机 Linux release 验证

2026-10-02，从提交 `7c3797261960ce6d636ee062049b01f50d9b99c6` 的 `git archive` 快照 `/tmp/cocobeat-linux-release` 执行以下命令，退出 0；环境为 CachyOS x86_64、Rust/Cargo 1.98.1，构建身份为 `0.1.0-7c3797261960-linux-release-probe`

```sh
CARGO_TARGET_DIR=/home/lzzz/MyProjects/CoCoBeat/target \
COCOBEAT_BUILD_ID=0.1.0-7c3797261960-linux-release-probe \
cargo build --offline --locked --release -j1 -p cocobeat-game
```

复用该提交 Linux workflow 的打包步骤生成真实 tar.gz，ELF x86-64、可执行权限、8 种 hicolor 图标、desktop 文件及 BUILD-INFO 均通过检查；独立解包后的 CLI 与实际 GPU 离屏启动检查见 [验证策略](testing.md#原生-linux-release-证据)

证据目录为 `target/linux-release-evidence/7c3797261960/`，保存构建/打包日志、源码与工具链身份、`acceptance.json`、Replay 结果、GPU 截图以及 `readelf`/`ldd` 输出

| 产物（相对证据目录） | SHA-256 |
|---|---|
| `dist/package/bin/cocobeat-game` | `6f494ba6b8b4d929461069603ca994a7ca89e07ddcaffb3a22de144d911b587e` |
| `dist/cocobeat-x86_64-unknown-linux-gnu.tar.gz` | `5b49a9c0645def2ddd4dc45124a67456bd4b383082d334dca9be5b849a7fa88f` |

本机 `ldd` 全部解析成功，但产物最高要求的 glibc 符号为 `GLIBC_2.44`；这是 CachyOS 本机构建证据，不能证明 Ubuntu 24.04 基线兼容，远端 Ubuntu 构建仍为 NOT RUN，不能将此包作为该基线的发行产物

## 跨平台交付门槛

初版面向 Windows MSVC 和 Linux 的 x86-64 / ARM64。发行前对每个平台/架构分别执行完整 workspace 检查、release 构建和干净机器运行，并记录 OS、GPU、音频/手柄后端与实际帧时间数据。
轻量 CI 与手动构建服务开发效率，不降低真实手柄和真实声音输出的验收要求。原生 ARM64 runner 构建通过后，仍需在真实 ARM64 游戏设备上验证图形、音频和输入。
