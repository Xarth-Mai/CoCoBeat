# CoCoBeat 品牌资产与启动模块

长字标以用户提供的定稿 JPG 描摹，启动动画和正式标题共用 Bevy 实体、mask 与 WGSL 材质；方形 Symbol 仅用于 README 和程序图标

## 资产

- `wordmark.svg`：840×180 逻辑画布，五组手工 Bézier 路径；SVG 预览渐变便于检查轮廓，运行时颜色以 shader 为准
- `masks/`：五张 3360×720 RGBA 白色覆盖 mask，共用画布和坐标，覆盖度位于 alpha
- `shaders/brand.wgsl`：字标染色、固定高光、Beat 蓝粉晕染与软边颜料球
- `icons/symbol-source.png`：参考图经 OpenAI image_gen 编辑的静态源图，保留角色和三颗颜料点，移除展示背景并调整深蓝底
- `icons/symbol-1024.png` 和各尺寸 PNG：静态母版及平台尺寸，圆角外为透明像素
- `icons/cocobeat.ico`：16、32、48、64、128、256px；`icons/favicon.ico`：16、32、48px

从仓库根目录重新导出，所需工具只参与资产制作，不进入游戏运行依赖

```sh
python3 assets/brand/export_wordmark.py
bash assets/brand/export_icons.sh
```

mask 导出需要 Python 3 标准库和 `rsvg-convert`，图标导出需要 ImageMagick `magick`；源 JPG、生成图和描摹结果不宣称逐像素一致

## 主线程接入

品牌线程只拥有 `brand_intro.rs`、`brand_audio.rs` 与本目录；共享文件由主线程统一落地

在 runtime 的 `lib.rs` 注册 `mod brand_intro; mod brand_audio;`，仅在正常游戏启动路径安装 `brand_intro::install(&mut app)`；保留现有唯一相机和 Kira manager，Replay、音频探针与旧 `--visual-smoke` 路径不自动播放片头

品牌模块使用当前已启用的 Bevy UI、render、asset、PNG 功能，不要求修改 runtime 依赖；所有 mask 与 WGSL 以固定 `embedded://cocobeat_brand/` 路径内嵌，可执行文件不依赖仓库工作目录

| 接口 | 主线程职责 |
|---|---|
| `BrandIntroLayout { dock_rect: Rect }` | 写入相机视口内左上角起算的逻辑 UI 像素矩形，模块等比适配并居中；默认值仅供独立预览 |
| `BrandIntroControl` | 失焦设置 `suspended`；仅主菜单显示时开启 `idle_enabled`，默认关闭；音效暂停/恢复也由音频所有者处理 |
| `BrandIntroStatus` | 读取 `phase`、`elapsed_seconds`、`idle_seconds`、`reveal_progress`、`error` 与 `is_complete()` |
| `BrandIntroSystems::Advance` | 在其之前更新布局和焦点状态，在其之后读取状态、消费落点消息、更新输入门控和 HUD |
| `BrandImpact::{Co1, Co2, Beat}` | 三次落点消息，使用 `MessageReader<BrandImpact>` 消费 |
| `brand_audio::sound(impact)` | 启动前各生成一次并缓存，用现有 Kira manager 播放 |

建议将主线程消费状态与消息的 `Update` 系统设为 `.after(BrandIntroSystems::Advance)`；UI 输入门控必须从 Loading 开始生效，不等待第一帧落点

### 启动和输入

阶段为 `Loading → Playing → Docking → Complete`；资源或 shader 编译失败进入 `Failed` 并携带错误，主线程负责明确显示错误与退出路径，不将失败视为 Ready

Loading 期间保持黑底，材质节点以零 alpha 参与准备；五张纹理、材质绑定与品牌 GPU 管线全部就绪后，先完整显示一次 t=0 白字标，随后独立呈现时间开始累计

动画期间与 0.45 秒停靠期间屏蔽菜单、歌曲、重绑定及游戏操作，窗口关闭始终有效；输入层继续维护设备、焦点和释放状态，完成时清空待处理操作，已按住的键或手柄按钮必须释放后再次按下

Complete 后保持现有 Ready，用户另行确认才启动歌曲；不提供跳过或自动重播，品牌模块不读写 SongTime、规则引擎或 Replay

### 时间和画面

| 秒 | 表现 |
|---|---|
| 0.00 | 白字标，无眼睛 |
| 0.40 / 0.50–0.75 | 白球下落，提前转蓝，落地前成为蓝球 |
| 0.90–1.12 | 撞第一个 C 的冠顶，从落点扩散蓝色，第一声 pon；第一组眼睛睁开 |
| 1.15–1.55 | 第一段弹跳，球的下半部提前转粉 |
| 2.10–2.32 | 撞第二个 C 的冠顶，从落点扩散粉色，第二声 pon；第二组眼睛睁开 |
| 2.35–2.75 | 第二段弹跳，提前形成蓝白粉 |
| 3.35–3.80 | 融入 B，Beat 完成扩散，whoom；眼睛暂时保持原视线 |
| 3.80–4.00 | Beat 染完后两组眼睛向内看，达到对视过冲位置 |
| 4.00–4.12 | 视线退回行程的 90%，轻微回弹 |
| 4.12–4.42 | 保持对视 |
| 4.42–4.95 | 从对视缓缓回到原定稿眼神 |
| 4.95–6.15 | 中央定稿保持完整 1.20 秒 |
| 6.15–6.60 | 同一 Logo 移动至 dock_rect，尺寸先缩到目标 95% 再回弹；背景退去，reveal_progress 从 0 增至 1 |
| 6.60 以后 | 同一材质及实体留在标题位置，主菜单可开启眼睛循环 |

两个 C 的碰撞点分别为逻辑画布中的 `(90,10)` 与 `(318,11)`，球按 SDF 可见下缘接触冠顶；球轨迹、飞溅和染色共用同一组接触点，避免落球位置与染色来源分离

下落与两段弹跳使用 Bevy 原生三次贝塞尔曲线，控制点按逻辑画布设置；晕染半径使用 `cubic-bezier(0.22, 0.75, 0.30, 1.0)`，撞击后先快速铺开再逐渐减速，shader 继续柔化扩散边缘

移动和界面显露使用 `cubic-bezier(0.45, 0.0, 0.20, 1.0)`；缩放在停靠前 72% 时间用同一曲线到达目标尺寸的 95%，后 28% 平滑回弹到 100%，过冲幅度按目标尺寸计算；独立预览通过同一个 `reveal_at` 函数采样，时间范围为 6.15–6.60 秒

两组眼睛独立控制睁开程度和水平视线，过冲限制在各自 o 的黑色眼窝内；4.95 秒后开场视线偏移严格归零，最终外观与首版定稿一致

品牌层使用 `GlobalZIndex(1000)`，主线程为它保留该 UI 层；HUD 根据 `reveal_progress` 显露并为停靠矩形留出位置，现有普通文本标题由主线程移除；单个活动 UI 相机是接入前提

主线程使用统一的 UI 逻辑像素尺度；修改 Bevy `UiScale` 时须同步换算视口和停靠矩形，当前模块按默认 `UiScale(1)` 处理，OS 高 DPI 由 Camera 的逻辑视口负责换算

三次落点使用越界检测，每个时间点正常播放最多发出一次；低帧率跨越多个落点会按时间顺序发送，主线程不得宣称此软件消息已证明扬声器与画面的硬件同步

### 主菜单眼睛循环

`Complete` 且 `idle_enabled` 为 true 时，独立的 `idle_seconds` 在 24 秒周期内累计；开场的 `elapsed_seconds` 固定在 6.60 秒，`is_complete()` 与 `reveal_progress` 不回退，循环只改变眼睛，不移动字标或发音效

| 菜单停留秒数 | 动作 |
|---|---|
| 0–2.80 | 正常视线，静止 |
| 2.80 / 2.89 | 蓝眼先眨，粉眼延后 90 ms 跟随 |
| 6.00 / 6.16 | 蓝眼先向内看，粉眼随后对视；轻微过冲回弹，短暂停留后约 8.01 秒回正 |
| 10.50–11.08 | 粉眼轻轻双眨，蓝眼在中间单眨 |
| 15.00 / 15.18 | 这次由粉眼先发起对视，蓝眼跟随；回弹后保持，约 17.28 秒回正 |
| 19.60 / 19.68 | 错开 80 ms 的轻眨 |
| 19.92–24.00 | 正常视线，静止；周期首尾姿态一致 |

失焦时冻结循环，恢复首帧跳过跨越失焦期的 delta；退出主菜单时将 `idle_enabled` 设为 false，视线恢复定稿并清零循环时间，下次进入菜单从静止段开始；主线程具体接线见 [HANDOFF.md](HANDOFF.md)

### 音频

三个 48 kHz 原创合成音分别长 280、320、620 ms，第三声正常情况下在 3.97 秒结束；主线程持有独立启动音效句柄，不占用歌曲句柄

主线程处理播放失败、设备错误、失焦暂停/恢复和退出时停止；品牌模块仅产生确定性 PCM 与语义消息，不创建 AudioManager，不改现有音频错误路径

## 静态图标与发行交接

README 引用 `assets/brand/icons/symbol-256.png`，本轮不在游戏画面中增加 Symbol

Windows：主线程在 game 应用构建脚本中通过 `.rc` 引用 `cocobeat.ico`，建议使用 `embed-resource` 构建依赖并对资源编译失败返回错误；由主线程核对版本、更新 Cargo.lock，分别验证 x86-64 / ARM64 PE 图标资源

Linux：发行包携带 PNG 和名为 `cocobeat.desktop` 的 desktop entry，约定 `Name=CoCoBeat`、`Exec=cocobeat-game`、`Icon=cocobeat`、`Type=Application`、`Categories=Game;`；将 PNG 放在对应的 hicolor 图标尺寸目录，提供用户级安装说明以解析可执行文件路径，不在品牌交付过程中安装系统文件

窗口图标：主线程使用对应平台的 winit 窗口接口；Wayland 设置与 desktop entry 相同的 app_id，图标依赖桌面环境查找，不能以 Windows 或 X11 的成功推断 Wayland 图标已生效

发行包包含 README 所引用的图片路径与本目录来源说明；方形 Icon 的外部来源再分发状态见 PROVENANCE.csv，主线程按仓库现有资源许可规则处理

## 验证

独立验证程序位于 `validation/`，不会注册到生产 workspace；实际命令、输出路径、结果和验收边界见 VALIDATION.md

本轮未接入生产启动、输入、音频管理器、README 或发行 workflow，共享文件的注册和端到端验收仍属于主线程
