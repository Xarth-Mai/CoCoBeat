# 原生场景性能观察

2026-10-08，实际 Linux x86-64 / AMD RX 6650 XT / RADV Vulkan / CPAL ALSA，以原生产优化 profile 构建游戏，445 项源码与构建输入前后相同，固定 binary SHA-256 `26ecc90318f6c937edffb63d698a59ce28c973fc44a6f947b2e2ee46d217d7c3`

完整开场后 Ready 预热 5 秒、采样 10 秒，明确确认后 Running 预热 10 秒、采样 50 秒；使用原 64 秒生产包、真实 Kira 游标及原 Session/core。独立有界 worker 在实际 ClockBridge 发布上预约并记录真实 Instant，呈现随后消费原捕获时间，未用预定时间替代实际输入或放宽等级门槛

## 结果

原环境三尺寸 × 四画质无限制共 12 项，加 1920×1080 medium limited60 与 VSync，共 14 项：13 VALID，1 Wayland VSync 超时 INVALID；原最慢有效 limited60 在新进程两次复测 VALID。另一个隔离 X11 VSync 对照完整运行 VALID，后端环境范围独立，原矩阵失败保留

| 尺寸 | low p95 / p99 ms | medium p95 / p99 ms | high p95 / p99 ms | off p95 / p99 ms |
|---|---|---|---|---|
| 1280×800 | 1.832 / 2.071 | 3.409 / 3.885 | 3.459 / 3.938 | 1.820 / 1.946 |
| 1920×1080 | 1.845 / 2.015 | 3.579 / 3.945 | 3.585 / 3.950 | 1.824 / 1.940 |
| 2560×1440 | 1.847 / 1.982 | 3.428 / 3.857 | 3.455 / 3.853 | 1.830 / 1.969 |

limited60 原轮与两复测 Running p95 / p99 分别为 16.732 / 16.737、16.734 / 16.737、16.731 / 16.737ms；独立 X11 VSync 为 33.320 / 33.673ms，aggregate main-update 为 60.0199Hz，这些都不是显示器呈现 FPS

16 次有效运行各有 13 次真实捕获，P1 / P2 接收 Hit 7 / 6、FreeSync 1、AnchorSync 5，两人的 Anchor 计数均为 Precise 2 / Good 3 / LateOrEarly 0 / Miss 2，并覆盖 Curve / Bridge / SectionCue。真实 game 与 wrapper 均 exit 0，自有 PID / 进程组退出；无限制组峰值 RSS 为 388828–566752KiB，含解码 PCM 与探针行缓存，不等同无探针稳态 heap 或 VRAM

原 Wayland VSync 游戏在 180 秒期限退出 -15，没有最终 frames / result；探针只在结束时写 RAM 行缓存，缺少最终文件不能证明零更新或确定冻结阶段。具体根因尚未建立；X11 补测明确记录实际 winit X11 后端及移除 Wayland 环境变量，不能替代原环境失败。运行期间 thread wchan 的等待状态也不能确定冻结原因

## 复现与范围

使用[测量工具](../tools/runtime-performance-check/README.md)，正式 runner 与 summarizer 原字节 SHA 已冻结；旧未使用的 rustc helper 已从正式工具退役并保留在 target 原始归档中，当前 source / build 身份与历史测量时工具身份分别记录

实际构建命令为 `cargo build --locked -p cocobeat-game --release -j 2 --message-format=json`，opt-level 3、fat LTO、一个 codegen unit；未降低优化配置。窗口逐个运行，记录 DISABLE_GAMESCOPE_WSI=1 本地 QA 条件并保留旧外部 WSI 清理崩溃，不改变产品默认呈现策略

完整数值、构建输入、原始命令、失败、归档映射与 SHA 索引见[持久观察](../testdata/synthetic/runtime-performance-observations-20261008.json)，原始产物位于 `target/performance-delivery-20261008/`。历史 frame-polled release 与 debug 行为证据单独保留，没有与当前 fixed release 合并

本批没有 universal 性能预算；GPU elapsed、draw call、VRAM、audio underrun、实际显示器帧、真实键盘 / 双手柄 / 混合输入、DAC / 扬声器、四平台图形与真人验收仍 NOT MEASURED / NOT RUN
