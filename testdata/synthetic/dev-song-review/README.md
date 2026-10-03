# 原创开发曲待审阅清单

[review.json](review.json) 从既有 64 秒原创音乐的制作配方枚举 438 个声部起点，合并为 200 条同帧来源候选，并保留 7 个结构边界和 7 个原创作 Anchor 引用；未运行 MIR、生成音频或完成人工听感与可玩性验收

输入为 `target/dev-assets/cocobeat-64.wav` 的既有 48 kHz、双声道 PCM16，SHA-256 为 `3390dd080cb536fd4a598ea933618dd99b4bf697c0b2874ff5cb0e220feb09e8`；生成器还绑定实际配方源码与两个 CSV 的 SHA-256，来源漂移时拒绝输出，详细身份在 JSON 的 `inputs` 中

时间线、音乐和原 Anchor 使用 CC0-1.0，[生成器](generate.py) 使用 MPL-2.0，沿用[原创内容来源与许可](../../../assets/dev/vertical_slice/README.md)；本清单由 CoCoBeat contributors with OpenAI Codex assistance 整理，没有引入外部音频、标注或算法依赖

## 审阅语义

- `source_frame` 是配方触发帧，`voices` 保留同帧的 kick、low、motif、pad、hat 来源；`onset_review.frame`、半开区间 `interval` 与 `reviewer` 全为 null，`status` 为 pending，不能把来源帧复制为人工真值
- `anchor_review` 的 `status` 为 pending，`playable` 和 `reviewer` 为 null；现有 7 点以 `existing_authored_anchor` 身份、原 ID 和原帧引用对应候选，已有创作决定不表示音乐或双人可玩性验收通过，200 个候选不自动扩成 200 个 Anchor
- `structural_boundaries` 单独保存段落开始与独占 EOF；40–48 秒精确静音内部无候选，frame 3072000 是文件结束边界而非有效样本或攻击标签
- tone 的攻击因子在 240 帧即 5 ms 到顶，三角波从 0 开始且存在整数取整；触发帧、首个非零、可听起音与谱峰时间不同，不能从混音是否非零反推独立声部起音
- hat 在左右声道符号相反，本配方 hat 启用时淡出系数为 1，均值下混会抵消该声部；人工审阅先听实际立体声，可独听两声道辅助定位，记录播放方式，不把下混丢失的声部当成立体声无声

先听完整段落，再逐条记录 accepted / rejected / uncertain、帧或不确定区间和理由，允许新增未由配方清单覆盖的可听变化；MIR 输出不参与初判。预先固定子集由两位真人独立审阅并保留分歧，Anchor 可玩性另行确认；原创曲仍是开发材料，不能当外部音乐盲测或替换旧 34 项控制及其 FAIL

## 复现

需要上述既有 WAV；从仓库根运行以下命令，生成器只读取并验证输入，不合成 PCM、不执行 Cargo，输出路径必须尚不存在

```sh
mkdir -p target/dev-song-review-20261003
python3 testdata/synthetic/dev-song-review/generate.py target/dev-song-review-20261003/review.json
cmp testdata/synthetic/dev-song-review/review.json target/dev-song-review-20261003/review.json
```

脚本内直接检查声部/合并计数、顺序、静音与 EOF 边界、原 7 个 Anchor 帧及引用。人工审阅应另存工作副本，保留本初始清单及来源；输出独占创建，已有文件不会被覆盖
