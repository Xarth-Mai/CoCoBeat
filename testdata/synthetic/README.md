# 软件计时实验

`cargo run --locked -p cocobeat-lab -- timing-sim` 导出 `target/timing-sim/samples.csv`、`summary.csv` 与环境说明；可选目录参数改变输出位置，所有记录均标记 `SIMULATED`

实验覆盖 30、64、300、600 秒，每个长度运行理想时钟、+100 ppm、−100 ppm、带采样误差与渲染卡顿四种情况；真值由独立的整数设备振荡器给出，ClockBridge 每 50 ms 获取观察，每 20 ms 映射一次模拟输入入口

采样误差按 0、+32、−32 frames 循环，声明 96 frames 观察不确定性；卡顿为每 3 秒一次 120 ms，单独保留最早入口与延迟消费时间，render 列展示误用消费时刻产生的偏移

summary 的 p50/p95/p99 为绝对映射误差的 nearest-rank 分位数，单位为 canonical frame；每 48 frames 为 1 ms，最大不确定性、越界数和残余漂移分别导出，任何误差超过不确定性都会令命令失败

这是快速、确定性的软件实验，模拟的 600 秒不要求墙钟等待 600 秒；真实音频输出、loopback、键盘/手柄延迟和平台设备切换验收均未运行，不应从这些结果推导实际硬件精度

## 实际音频探针

`cargo run --locked -p cocobeat-lab -- audio-probe 30 target/audio-probe-30` 显式启动 Kira 点击音播放并采样软件播放游标，时长只接受 `30`、`64`、`300`、`600` 秒，输出目录为必填参数

此命令会在默认音频设备实际发声并等待指定时长，不由测试或其他 lab 命令自动执行；输出 `expected-clicks.csv`、`cursor.csv`、`metadata.txt`，预期点击位置与软件游标观察用于后续硬件实验对照

软件游标不是 DAC 输出时间，报告保留 `NOT MEASURED` 的物理延迟状态；需要另行采集 loopback 或外部设备信号才能测量真实输出偏移，当前尚未运行此硬件探针

## 雨夜表现软件观察

[2026-10-09 表现观察](visual-polish-observations-20261009.json)绑定七份生产源码、冻结 debug / release、196 项运行时测试、13 张 GPU 图、240 帧连续画面和一个实际 Kira 原生性能样本，软件与限定画面验收不代表商业质量、物理输入或显示器性能验收
