# 09 · 编辑器与 Replay

前置：已有编译内容和 02 的基础 Replay。此时创建 cocobeat-editor。

- [ ] 显示波形、精确 SongTime、音乐结构、Anchor 候选/证据与拒绝原因。
- [ ] 增删移动 Anchor，支持撤销重做，重新生成内容哈希。
- [ ] 显示 Replay 输入、Anchor 判定、Free Sync 配对与计时诊断。
- [ ] 固定带版本/长度限制的序列化契约，校验损坏与不支持版本。
- [ ] 编辑导出再载入无损；同一 core headless 重放结果一致。

退出条件：可以根据事实定位问题，Replay 不包含用户音频，开发遥测默认留在本机。调试 UI 不自动变成正式游戏 UI。
