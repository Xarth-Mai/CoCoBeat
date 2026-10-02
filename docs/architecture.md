# 架构与依赖边界

箭头表示“依赖”，不表示运行时消息方向：

```text
game ───────────────→ runtime
runtime ────────────→ schema / core / replay
replay ─────────────→ schema / core
core ───────────────→ schema
schema ─────────────→ std
lab ────────────────→ 按实验需要使用上述模块
xtask ──────────────→ 开发检查工具
```

schema 定义整数时间、玩家/epoch/序号、Hit、水位和语义事件；core 的 DuoEngine 统一执行 Anchor 判定、一对一 Free Sync、Anchor Sync 与 Resonance；replay 保存带身份和版本的有界 JSON，再交给同一个 core 重放

runtime 的 app 组合 Bevy、Kira 和 Session；input 记录软件观察时刻，clock 映射 SongTime，session 维护规则事实、Replay 与诊断，audio 管理播放及错误，view 只消费表现状态；game 只调用 runtime 入口，lab 复用 clock 与原创 dev_song 生成器

## 未来模块何时出生

| 模块 | 独立责任 | 引入时机 |
|---|---|---|
| media | 解码、重采样、标准编码回读、MusicAnalysis、AnchorCompiler、导入事务 | 05 标准音频；06/07 逐步加入分析与编译 |
| stage | MusicAnalysis / 编译后的音乐结构 → StagePlan，确定性轨道和几何校验 | 08 自动舞台；手写场景先在 runtime |
| editor | 波形、Anchor、Replay 的可视化与人工修改 | 09 已有可编辑内容契约 |
| net | Quinn 传输、会话、时钟映射、可靠输入历史和资源一致性 | 10 本地闭环与重放通过后 |

media / stage / net 依赖 schema，不能依赖 runtime。算法以项目自有类型为输入输出，第三方库类型止于适配器。runtime 组合实现；game 只保留配置和启动，不承载算法。未来增加 crate 时必须说明责任、依赖和失败方式，并更新边界检查。

## 事实流

```text
Bevy 输入消息 → ClockBridge → Hit / 水位 → core::DuoEngine → 语义事件
                                   ↑                          ↓
                          Replay / 未来网络            app 声光反馈 → view
```

相同输入历史、规则版本、内容和 epoch 必须得到相同规则结果；Replay 不另写判定算法，未来网络到达时间不能改写原始输入时间，view 不获得规则引擎的可变控制能力

当前开发内容在 runtime 内确定性生成，尚未经过 canonical Ogg 导入管线；Replay 记录内容/规则/构建身份和原始输入、水位，保存失败明确报错；校验与大小上限由 replay 持有，诊断 CSV 由 session 写入，二者不包含歌曲音频

## 自动约束

`cargo xtask boundaries` 检查 Cargo metadata 中所有直接依赖声明，包括 build/dev、目标平台条件与重命名依赖。
schema / core 不允许外部依赖，replay 仅允许 serde / serde_json 处理私有持久化格式，且只能沿上图依赖；game 的运行时只允许 runtime，Windows 构建脚本允许 embed-resource 编译 EXE 图标资源，例外不扩展到普通、dev 或其它平台依赖。runtime 和 lab 可接入第三方实现依赖；未声明的本地 helper 不得绕过边界
需要 serde 等纯数据工具时，应显式更新白名单并说明用途，不能泛化为允许任意第三方依赖。

边界检查约束模块图，不能证明所有函数都尊重语义；例如反馈不修改判定，还需要 API 设计、测试和代码审查。
