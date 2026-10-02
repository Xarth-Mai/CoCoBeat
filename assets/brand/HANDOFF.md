# 新开场和主菜单循环接入

交接给主线程 `codex://01a0fae8-5053-7782-838b-f3c96dd77d67`，共享文件仍由主线程统一修改

## 品牌模块已实现

落点仍为 0.90 / 2.10 / 3.35 秒；Beat 在 3.80 秒染完后眼睛才开始对视，视线有轻微过冲回弹，4.95 秒回归定稿；随后停留 1.20 秒，6.15–6.60 秒缩小归位，尺寸最低为目标的 95% 后回弹

启动完成后可开启 24 秒眼睛循环，包括错开眨眼、蓝粉轮流发起对视、回弹和回正；字形、颜色、位置与音效不再变化，循环首尾均为正常眼神

## 主线程接线

`BrandIntroControl` 增加 `idle_enabled: bool`，默认 false；`BrandIntroStatus` 增加 `idle_seconds: f64`，由品牌模块维护，主线程只需读取

在 `app.rs` 现有 `suspend_intro` 系统参数里加入 `game: Res<Game>`，每帧设置：

```rust
control.idle_enabled = game.phase == Phase::Ready && input.menu_open;
```

保留 `suspend_intro.before(BrandIntroSystems::Advance)` 的排序和原焦点、音效暂停逻辑；这使初始主菜单播放循环，开始歌曲后关闭，暂停和结算页继续使用静态定稿

输入解锁继续依赖 `is_complete()`；启动完成时间已由 5.45 秒延至 6.60 秒，需要同步主线程中任何固定时长断言或超时预期，仍须用户重新确认才能播放歌曲

不需要新依赖、Bevy feature、模块注册或音频 manager；现有 `brand_intro::install` 会安装循环，`idle_enabled` 是唯一新增接线开关

## 验证与待验收

品牌模块的窄测、真实 GPU 预览、回弹和循环接缝结果见 [VALIDATION.md](VALIDATION.md)；预览由实际模块和 WGSL 渲染

主线程接入后验证 Ready 菜单有动画、开始歌曲后眼睛回正、失焦冻结与恢复无跳跃、返回主菜单从静止段开始、输入门控和启动音效不受影响

`todo/planning.md` 的“开屏 Logo 动画”条目本轮未改写，待主线程接线与上述验收完成后再勾选；共享源文件及其他未提交改动均保留
