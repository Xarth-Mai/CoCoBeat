# 原生谱形变化与相似度诊断

`cocobeat-lab inspect-structure-features PACKAGE left|right NEW_REPORT` 只读输出最终 canonical 音频的粗谱形、相邻变化及相似邻居，供作者定位需要听审的位置；歌曲段落 / 重复关系、置信校准和自动舞台编排仍按独立音乐证据验收

```sh
cargo run --locked -p cocobeat-lab -- inspect-structure-features /path/to/package left /path/to/new-structure.json
cargo run --locked -p cocobeat-lab -- inspect-structure-features /path/to/package right /path/to/new-structure-right.json
```

输出父目录须已存在，报告须位于原包之外且尚不存在；复用完整四对象校验、同次 owned 音频快照、写出前 Source 回读和有界 create_new 保存，4 MiB 超限拒绝，已有输出保持。报告绑定 CID / audio BLAKE3 / canonical N / decoded basis，保存原 profile、48 kHz 双声道及显式左 / 右声道；本命令不修改包、Authoring、Anchor、SectionCue、Stage、Ready 或 Replay

## 固定特征与坐标

profile 为 `canonical-spectrum-1024x24-v1-candidate`，直接消费选中声道的真实样本，不下混、归一化、reflect 或 EOF 补零；复用已安装的 OxiMedia 安全 FFT，不调用 beat 模型或 SDK，不新增 GPL 或 Python 产品分析后端

1024 帧不重叠 Rectangle 窗按原 forward magnitude 取单边 bin `0..=512`，八个频带边界为 `[0,4,8,16,32,64,128,256,513]`；每窗先求各带 magnitude-squared 总和，再以真实完整窗数求均值并保存 `log1p`，不倍增单边功率。这些是固定规则的谱描述符，不是物理声压或音乐类别

每 24 窗构成一个 24576-frame bin，即 0.512 秒，区间为 `[i*24576,min((i+1)*24576,N))`。所有真实样本参与 f64 RMS / peak；不足1024帧的尾部只参与能量，另保留 `spectral_frames` / `partial_tail_frames`。没有完整窗时 `INSUFFICIENT_FULL_WINDOW`、谱字段为 null；零范数或缺谱的相似度不可用，不生成静默匹配

非零八维 log-power 描述符输出原始 cosine，相邻变化为 `1 - raw_cosine`；各 bin 从其他有效 bin 中保留至多四个最高分邻居，按 score 降序、原 index 升序稳定排序并排除自己。邻居可以相邻，分数不裁剪、不设音乐阈值，不拼接重复区间；整数 canonical 支持与原值保留，bin 端点不改称歌曲结构边界

报告 `confidence` / `beat_unit` / `meter` 为 null，`quality_status=UNASSESSED`、`production_admission=false`。`sections_context` 仅记录原 analysis sections 数量和原 capability；legacy 缺省保持 null，已有作者元数据不作为新描述符的音乐真值。只读 JSON 和原生审阅工作台共用这些原值，自动采用另行验收

## 只读结构工作台

`workbench-candidates PACKAGE --structure left|right [--locale CODE]` 用显式声道打开谱形诊断，复用完整包校验与同次 owned canonical PCM 读取，不要求预先生成报告。`--locale` 复用游戏支持的 13 个语言变体、Noto Sans 和现有主控 / 焦点规则

```sh
cargo run --locked -p cocobeat-lab -- workbench-candidates /path/to/package --structure left --locale zh-CN
cargo run --locked -p cocobeat-lab -- workbench-candidates /path/to/package --structure right --locale fr
```

列表保留原 bin 编号与完整 `[start,end)` 整数区间；时间线蓝色表示选中区间、紫色表示至多四个相似邻居、白色表示作者 Anchor / SectionCue，上下文来自现有谱面，不是盲标。详情保留原 JSON 的 RMS / peak、频带值、相似度、尾部状态、Source 与未知字段；相似区间不命名为音乐段落，也不自动生成 Anchor

点击按原半开区间选中，真实最后不足窗的尾部仍可浏览；EOF 不映射到任何 bin，光标到达 N 时保留原选中记录。编辑、拖动、撤销 / 重做和导出快捷键沿用只读门控，源包及原历史保持。试听复用实际 Kira 生命周期，明确播放才创建输出，分析的 left / right 选择不改变原立体声播放

本轮生产 media 73 项 / Lab 85 项与 i18n 3 项测试、两包全目标 Clippy、格式、workspace 边界和 Lab 构建通过。9 条实际生产 CLI 控制为 7 次预期拒绝及左右两次旧诊断回归；左右报告与原快照字节相同、原包四对象保持，这些命令不计为原生窗口正向验收

两个原生窗口通过：中文 1280×800 的125区间浏览、详情滚动、软件键盘＋双手柄主控 / 失焦门控与实际 Kira 播放 / 暂停 / 停止，法语 640×480 的两区间、真实一帧尾及 EOF；六张实际 PNG 均经主线程与独立审查逐张目检。原包、谱面与372项构建输入保持，原始分析值与旧报告的 binary64 数值精确一致，见[工作台观察](../testdata/synthetic/structure-workbench-observations-20261008.json)

首轮两个窗口 FAIL 保留：未启用 float_roundtrip 的 QA JSON 解析产生462 / 3处一 ULP 读回差异；仅修正 target 验收脚本，统一解析路径并另核全部原始 binary64 叶值，生产算法、原报告和容差未改。软件 callback / 注入输入不证明声学输出、物理手柄、鼠标一帧精度或真人音乐参考，十分钟 GUI 成本与新功能四目标原生构建仍未验

## 软件观察与成本

完整 499 文件候选快照完成 media 5 项 / Lab 2 项窄测、两包全目标 Clippy、格式及 debug Lab 构建；499 由 492 项生产基底、3 项测试 fixture、2 项已有 example 和 2 个新增模块组成。首次 497 文件快照遗漏两个已声明的 native Vorbis example，Clippy exit101 保留；只补源闭包后通过，候选算法不变。以下 CLI 结果绑定该快照冻结 Lab，不能改绑为后来生产二进制的实际运行

此前 JSON 入口接线（`a612ee1`）的四个 Rust 文件与候选逐字节一致；该批生产 media 72 项 / Lab 82 项测试、两包全目标 Clippy、格式、workspace 边界、Lab 构建及一次实际 short-left CLI 均通过，六条命令均 exit0。该批生产 Lab 的 short-left 报告与候选快照逐字节相同、原四对象保持，见[生产检查记录](../testdata/synthetic/structure-feature-production-checks-20261008.json)；12条完整 CLI 仍绑定原快照，新命令四目标原生验证仍为 NOT_RUN

12 条实际 CLI 为 2 次手工导入、6 次正向诊断和 4 次预期拒绝，分别为 8 次 exit0 / 4 次 exit1；覆盖显式左右、既有32秒 / 64秒包、真实短尾、600秒非静音包、非法声道、已有输出、包内输出和损坏音频身份。各命令预先限定180秒、无超时，原包 / 输入及冻结身份保持，命令与原始报告见[观察记录](../testdata/synthetic/structure-feature-observations-20261008.json)及[原始证据索引](../testdata/synthetic/structure-feature-observations-20261008-raw-index.json)

真实短尾 N=24577 的左右报告各保留 `[24576,24577)` 最后1帧非零 RMS / peak，谱为 null、邻居为空、相邻相似度不可用；600秒左声道 N=28800000 产生1172个 bin / 28125个完整窗，本机 wall 45.286秒、单子进程峰值 RSS 71352 KiB。成本来自一次 Linux x86-64 debug 快照进程，包含包读取 / 特征 / 保存路径，不是发行预算、跨硬件保证或新增特征常驻内存上限

报告的有限性、原半开区间、尾部支持、log1p 及全部有效候选的 top4 以预先声明的数值容差复核；CLI 保存 BLAKE3 来自实际 stdout，独立报告字节核验使用 SHA256，未冒充独立 BLAKE3 重算。旧64秒来源总报告的 GPU 不可用 FAIL 与首轮快照失败保留；本批 CPU 诊断不改写旧视觉结果

旧 wholeSpect 19 PASS / 9 FAIL（整体 FAIL）与音乐质量 FAIL 保留，描述符机制检查不关闭完整 MIR、可靠 section / repetition、Anchor 校准或舞台自动编排。新入口四目标原生构建、发行 CPU/RSS 预算、窗口 / 声学 / 设备及独立真人结构参考继续按各自证据验收，既有 `8cdda9d` 四目标结果只覆盖其原功能版本
