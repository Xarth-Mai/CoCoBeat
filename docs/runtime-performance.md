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

## 新版分项测量接线

2026-10-09 · 现有 opt-in 探针复用 Bevy 官方渲染诊断，20Hz导出最多64个路径的最新原始CPU记录 / GPU查询值、f64 bits与主世界接收时间；官方插件每个渲染帧仍记录，接收时间不提供渲染frame ID，分项不合计成完整GPU帧或归因到歌曲阶段

Linux runner新增进程累计CPU、largest waited-child peak RSS和观测launch-to-reap耗时，采用0.2秒名义轮询周期，未测内核精确退出时刻，包含被中间父进程wait的后代CPU；这与并发进程树RSS、稳态heap、VRAM和声卡underrun分别记录范围

五项producer窄测、runtime Clippy / 格式 / 边界通过；固定 `5874f59` 的375项构建输入和优化Game已完成构建，旧矩阵、Wayland VSync失败及所有旧数值保持原构建范围

## Stage 3 分项原生观察

2026-10-09 · 原64秒包、1280×800 medium unlimited在Linux x86-64 / RX 6650 XT / RADV Vulkan上完成一次实际运行，Game exit0；Ready采样10秒、Running采样50秒，Running主更新间隔p95 / p99为3.666154 / 4.136922ms，13次独立Instant捕获完成，详见[原始观察与身份索引](../testdata/synthetic/runtime-performance-stage3-observations-20261009.json)

观测Game生命周期88.519560秒，wait计账CPU169.226397秒，largest waited-child peak RSS604416KiB；并行线程CPU可以超过wall，这些数值包含启动、解码与探针，不能推断稳态heap、并发进程树RSS或音频underrun

原始GPU查询记录覆盖十种渲染路径，20Hz周期采样保留原f64 bits；main opaque pass的p95为1.14108ms，bloom为0.49872ms，UI为0.31584ms，各自是分项查询，不相加为完整GPU帧，也不按延迟接收时间归因到歌曲阶段

首次case在Game启动前失败：Python的platform.platform实际启动uname -p，污染零子进程资源基线；runner改用os.uname读取元数据，原guard和全部预算保持。原FAIL、纯CPU根因trace与独立零用量窄测保留，修复后的QA工具身份与已完成的5874f59 Game构建身份分别冻结，未重建Game

本次单样本为描述性软件证据；原Wayland VSync失败、四平台图形、物理输入、DAC /扬声器、真人和完整性能预算继续分别验收
