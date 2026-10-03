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

`.github/workflows/ci.yml` 在 main 的代码 push 与涉及代码/构建配置的 PR 执行，也提供 `workflow_dispatch` 手动入口和供发版复用的 `workflow_call`：

1. 一个 Ubuntu job，安装最新 stable，恢复按工具链、manifest/lockfile 与源码区分的缓存。
2. 全 workspace 的 rustfmt 与依赖图检查。
3. schema/core/replay/media/xtask 的 Clippy 和测试

无图形/音频运行层编译、无平台矩阵、无自动 release 打包；新提交取消旧检查，单次限制 10 分钟。只改文档或未列入触发路径的资源不会触发当前自动 CI；嵌入运行时的 `assets/i18n/`、`assets/fonts/` 和 `assets/flags/` 变更会触发轻量检查，国际化运行时行为仍由完整检查与 GPU 验证覆盖。完整检查仍用 `cargo xtask check`，新增非 Rust 源码或构建输入时同步更新触发路径。

CI 与四目标发行构建均使用 `actions/cache@v6`，保存 Cargo registry/git 和对应 debug/release 编译目录；发行缓存按 OS、target 和 Rust 版本隔离。先匹配 manifest/lockfile 与源码，未命中再尝试同依赖版本，最后回退到同平台和工具链的旧缓存，Cargo 仍执行原有 `--locked` 检查与构建，重新编译受影响的内容。tag 发布复用同一构建工作流，可读取默认分支的缓存；不同 tag 之间的可见性遵循 [GitHub 缓存作用域](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#restrictions-for-accessing-a-cache)

2026-10-03 的只读远端核查确认已保存 9 条缓存，共 4,498,931,681 字节，包含四个发行目标；既有六次发行 run 都是未命中后成功保存，本次只据此确认缓存存在，尚不宣称新增回退已命中或节省多少构建时间。核查记录见 `target/github-readiness/cache-audit-20261003.json`

## 手动发行构建

`.github/workflows/release-build.yml` 提供单目标 `workflow_dispatch` 和可复用的 `workflow_call`，普通 main 提交不会自动编译发行包

打开 **Actions → Release build → Run workflow**，选择分支和 `target`：

| 目标 | 原生 runner | 产物 |
|---|---|---|
| `x86_64-pc-windows-msvc`（默认） | windows-latest | 含 x86-64 Windows EXE 的 ZIP |
| `aarch64-pc-windows-msvc` | windows-11-arm | 含 ARM64 Windows EXE 的 ZIP |
| `x86_64-unknown-linux-gnu` | ubuntu-24.04 | x86-64 Linux tar.gz |
| `aarch64-unknown-linux-gnu` | ubuntu-24.04-arm | ARM64 Linux tar.gz |

四个目标都是 64 位，不提供 32 位选项。一次手动运行只构建选择的目标；另一平台的 job 会跳过；下述 tag 发布通过矩阵分别调用四个目标。Linux 固定 Ubuntu 24.04 作为构建基线，x86-64 本地容器和四目标原生 runner 的构建结果见下文

工作流需先出现在仓库默认分支，GitHub 才会提供手动运行入口。原生 runner 安装目标、执行带 lockfile 的优化构建，然后核对 Windows PE 或 Linux ELF 的架构字段，避免错误标记产物架构。
成功后上传 `cocobeat-<target>-<commit>` artifact，保留 14 天，包含可执行文件、LICENSE、README、Cargo.lock 与 BUILD-INFO（提交、目标、工具链、profile、文件 SHA-256）。Windows 显式生成 ZIP，Linux 打包 tar.gz 保留执行权限；单目标手动构建只上传 Actions artifact，不发布 GitHub Release

Windows 与 Linux 包均带入 `licenses/`、品牌来源说明与静态图标，以及完整的 `assets/fonts/`、`assets/flags/`；六份 Noto Sans 字体和 13 组 SVG/PNG 旗帜随各自的 `README.md`、`SOURCES.json` 及 OFL/MIT 许可一起分发，来源文件的原字节哈希按资源总台账核验，运行时多语言的软件验证见 [验证策略](testing.md#国际化与字体里程碑)，四平台首次构建与下载包核验结果见下文，真实游戏设备的运行验收仍需分别执行

目前入口为本地 64 秒双人原型，包含程序生成的音乐与场景；第三方 notices、安装包、运行库与完整资源打包仍属于 V1 加固门槛，后续门槛按 [工作进度](../todo/progress.md) 推进，不能用本机编译推断其他平台或真实设备兼容性

## 推送版本 tag 自动发布

`.github/workflows/release.yml` 只由推送版本 tag 触发；有效格式为大写 `Vx.y.z`，例如 `V0.1.0`，各段不接受多余前导零或预发行后缀，tag 去掉 `V` 后必须等于根 `Cargo.toml` 的 `workspace.package.version`，每个 workspace 成员须继承该版本或显式声明相同版本

确认所需提交及版本已在仓库后，发布示例为：

```sh
git tag V0.1.0
git push origin V0.1.0
```

流程依次校验 tag 和 Cargo 版本、调用轻量 CI、调用四目标原生构建，最后下载恰好四个压缩包并核对各自 BUILD-INFO 的提交与 target；发布前再次确认远端 tag 仍指向触发构建的提交，再用 `gh release create` 创建带生成说明和四份附件的 GitHub Release，仅最后的发布 job 拥有 `contents: write`

版本、CI、任一目标构建或包身份核验失败时不会进入发布；没有手动发布 Release 的入口，单目标 `Release build` 的手动入口仍仅生成 artifact；本流程没有创建 tag 的步骤

附件命名为 `cocobeat-<tag>-<target>.<extension>`，例如：

```text
cocobeat-V0.1.0-x86_64-pc-windows-msvc.zip
cocobeat-V0.1.0-aarch64-pc-windows-msvc.zip
cocobeat-V0.1.0-x86_64-unknown-linux-gnu.tar.gz
cocobeat-V0.1.0-aarch64-unknown-linux-gnu.tar.gz
```

该新增流程的 actionlint 与 16 项本地行为矩阵通过，结果保存在 `target/tag-release-validation/behavior-results.json`，覆盖 tag 格式/版本、成员版本、压缩包数量/损坏/身份以及发布前 tag 移动；发布调用使用本地 `gh` mock，真实 tag 发布为 NOT RUN，下面已观测的手动构建不能替代本流程的远端验收

## 2026-10-03 四目标原生 runner 构建

下面记录使用当时的 `Manual release build`，Windows artifact 直接包含程序和资源文件；后续新增的显式 ZIP 与 tag 发布尚未由这些历史 run 验证

源码 `4c63dccad735fb2dd403e28fbf4b60f3e35fd548` 的四次手动 workflow 均完成 release 构建、架构核验和 artifact 上传，工具链实际为 Rust/Cargo 1.99.0，使用上述 fat LTO 发行参数

| 目标 | Actions run | 实际 runner image | 编译 | 下载包静态核验 |
|---|---|---|---|---|
| Windows x86-64 | [37082643568](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37082643568) | windows-2025-vs2026 | PASS | FAIL：来源文件被改为 CRLF |
| Windows ARM64 | [37082649929](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37082649929) | windows-11-vs2026-arm64 | PASS | FAIL：来源文件被改为 CRLF |
| Linux x86-64 | [37082654417](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37082654417) | ubuntu-24.04 | PASS | PASS |
| Linux ARM64 | [37082661755](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37082661755) | ubuntu-24.04-arm | PASS | PASS |

每个下载包的 BUILD-INFO 提交、target、发行 profile 和二进制 SHA-256 均匹配；两个 Windows EXE 为 PE32+，machine 分别为 `0x8664` / `0xAA64`，6 个内嵌图标图像逐字节等于源码 ICO；两个 Linux 二进制为 ELF64，小端 machine 分别为 `62` / `183`

Linux 包各有 62 个文件，60 个复制文件与该提交 Git blob 逐字节相同，35 项字体/旗帜来源大小与哈希通过，解包后可执行文件权限为 `0755`，8 个 hicolor 图标为 `0644`；最高直接 glibc 符号引用版本均为 `GLIBC_2.39`，其中 x86-64 对该版本为弱引用，不据此推断全部动态依赖或其他发行版兼容

首轮 Windows 包各有 52 个文件，其中 30 个复制文本经 LF→CRLF 转换，导致两份 Noto OFL、flag-icons LICENSE 和 13 个 SVG 共 16 项来源大小/哈希不匹配，首轮失败证据保留

提交 `34eaf5b2c4aa20a91a66bd85e34b5c39a48cbf0a` 用 `.gitattributes` 固定文本 LF，并保留 `assets/` 与 `testdata/` 原始字节；后续 Windows x86-64 [37084519612](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37084519612) 和 ARM64 [37084522075](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37084522075) 的原生构建与下载包静态核验均为 PASS，每包 50 个复制文件逐字节匹配该提交、35 项来源大小/哈希通过，PE 架构、BUILD-INFO、二进制 SHA-256 与 6 个内嵌图标图像均通过核验

本轮接受的 Linux 包来自 `4c63dcc`，Windows 包来自 `34eaf5b`；两提交仅差 CI 手动入口和换行规则，仍按各自源码身份保存证据，这些构建不覆盖后续 media 接入、显式 Windows ZIP 和自动 tag 发布

仅修改 CI 手动入口的后继提交 `0340ea954def3db68a2a6dff031036644f04d740` 自动触发 [Lightweight CI 37083062753](https://github.com/Xarth-Mai/CoCoBeat/actions/runs/37083062753)，fmt、依赖边界及当时 schema/core/replay/xtask 的 Clippy 和测试均通过；该结果不覆盖后续新增 media 的检查

持久观察记录为 [release-build-20261003.json](../testdata/synthetic/release-build-20261003.json)，保存提交、run、实际 runner、产物 SHA 与验收边界；完整日志和下载包保存在 `target/github-readiness/20261003-logs/`、`20261003-artifacts/`，过滤汇总为 `20261003-results.json`；本轮未执行这些下载二进制，真实桌面、GPU、音频、输入、干净机器和真人体验仍为 NOT RUN

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

本机 `ldd` 全部解析成功，但产物最高要求的 glibc 符号为 `GLIBC_2.44`；这是 CachyOS 本机构建证据，不能证明 Ubuntu 24.04 基线兼容，不能将此包作为该基线的发行产物

## Ubuntu 24.04 容器发行基线

2026-10-02，提交 `5949c13ce75e08d880648d72205259097e5e6ff0` 在官方 Ubuntu 24.04 x86-64 本地容器完成完整检查、fat LTO release 构建和既有 workflow 布局打包，均为 PASS；最高 glibc 符号要求为 `GLIBC_2.39`，`ldd` 全部解析成功，62 文件发行包独立解包后的 CLI 验证通过，命令、镜像 digest 和验收边界见 [验证记录](testing.md#ubuntu-2404-容器发行基线)

证据目录为 `target/ubuntu-build-evidence/e9345f9/`，目录名沿用准备阶段，实际构建源码为上述 `5949c13`

| 产物（相对证据目录） | SHA-256 |
|---|---|
| `artifacts/unpacked/bin/cocobeat-game` | `1e68c654f8368ed4bde7f9f9c68292641246795787622bd8fae9fdc0acb1b8fa` |
| `artifacts/cocobeat-x86_64-unknown-linux-gnu.tar.gz` | `be74360a7d0b09a9536ae2f87230da196d6d3b93f5d8005d4648d971b598adf9` |

## 跨平台交付门槛

初版面向 Windows MSVC 和 Linux 的 x86-64 / ARM64。发行前对每个平台/架构分别执行完整 workspace 检查、release 构建和干净机器运行，并记录 OS、GPU、音频/手柄后端与实际帧时间数据。
轻量 CI 与手动构建服务开发效率，不降低真实手柄和真实声音输出的验收要求。原生 ARM64 runner 构建通过后，仍需在真实 ARM64 游戏设备上验证图形、音频和输入。
