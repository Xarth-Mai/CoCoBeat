# 测试数据

时间、边界、规则和重放测试内置于相应 crate，通常不提交可由项目代码生成的媒体二进制 fixture；`synthetic/media-import/` 保留两个由独立编码器生成的微型原件，用于离线回归 MP3 声道/裁尾和 Ogg 损坏恢复边界，生成命令、来源与许可随文件记录，测试不需要 FFmpeg

[`synthetic/`](synthetic/README.md) 说明确定性软件计时实验；原创开发资源的独立规格在 `assets/dev/vertical_slice/event_frames.csv`，运行产物写入忽略的 `target/`

`golden/` 留待审阅后的固定预期，`human_labels/` 留待有权限使用的人工 Anchor 标注；生成输入和预期不能完全共享被测算法，至少部分真值采用独立规格中的帧号

Groove、Slakh 等外部数据集在 MIR 阶段记录具体版本、官方获取步骤、校验值和使用/再分发条款；核实之前不自动下载或提交。不要把用户导入的音乐当成可再分发测试数据。
