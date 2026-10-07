# Anchor 编辑与 Replay 工作台

`edit-anchors PACKAGE PATCH NEW_PACKAGE` 用整数音频帧修正已有 SongPackage 的 Anchor，并导出可重新加载的新包；当前支持增删、移动、撤销和重做，不修改分析事实、SectionCue 或判定规则，包格式继续使用 [SongPackage v1](song-package.md)

原生工作台与可重复执行的 CLI 共用 AnchorEditor 和保真导出；工作台显示立体声波形、整数时间线、Anchor 列表及作者设置的 SectionCue，并提供只读 Replay 与 Anchor 候选证据入口，试听校准继续后续。候选报告与明确采用见 [Anchor 提案](anchors.md)，原始输入、判定与配对的查看契约见 [Replay 诊断](replay-diagnostics.md)

## 原生工作台

```sh
cargo run --locked -p cocobeat-lab -- workbench /path/to/song-package /path/to/new-package
cargo run --locked -p cocobeat-lab -- workbench /path/to/song-package /path/to/new-package --locale zh-CN
```

源包完整验证后打开独立 Bevy 窗口，最小尺寸 640×480；这是静音内容工具，没有播放按钮。波形来自同一验证过程的真实立体声 PCM，每 64 帧保存左右声道峰值包络，最多 450,000 组；显示随视口聚合，整数帧编辑不受波形栅格分辨率影响

默认读取现有游戏配置中的语言，配置不存在则跟随系统，配置损坏会在 stderr 说明后使用系统语言；显式 `--locale` 使用游戏支持的 13 个完整语言代码并优先于配置。工作台复用 Noto Sans 与脚本回退，始终不保存或修改游戏设置

| 操作 | 输入 |
|---|---|
| 取得键鼠主控 | Enter 或鼠标单击，首次只接管 |
| 切换焦点 | Tab / Shift+Tab |
| 波形光标微调 | 方向键每次 1 帧，Shift+方向键每次 48 帧 |
| 缩放 / 平移 | 滚轮或加减键缩放，Shift+滚轮平移 |
| 选择 / 移动 Anchor | 点击波形标记或列表项，拖动标记；一次拖动只记一次撤销 |
| 精确输入帧 | 详情中的 Frame 字段输入整数，Enter 或 Apply frame 应用 |
| 新增 / 删除 | 工具栏在光标新增、删除选中项，Delete 删除选中项 |
| 撤销 / 重做 | Ctrl+Z / Ctrl+Shift+Z |
| 导出并关闭 | Ctrl+S 或 Export and close |
| 取消输入 / 关闭 | Esc；有未保存修改时默认选择 Keep editing |

窗口同时只有一个菜单主控，键鼠与具体手柄身份分开处理；手柄 Start 显式接管，方向键 / 左摇杆浏览、肩键切换焦点、South 查看、East 返回，编辑和导出需切回键鼠。普通副控输入不抢焦点，失焦或主控断开取消拖动，按住的输入须释放后才能再次生效；这些规则复用游戏菜单主控判定，不改变游戏 P1/P2 分配

密集重合标记显示选中像素内的 Anchor 数量，虚拟列表可逐个选中；详情显示作者原有 SectionCue 的 ID、帧和文字，不推断音乐强度或把提示当作判定点。光标可以查看 EOF，Anchor 只能位于结束帧之前；新项使用最小未用 ID，已有最大 u64 ID 不妨碍继续新增

导出在后台调用同一 `export_anchors`，期间保留窗口并暂停编辑和关闭；成功输出源 / 新包身份及路径后退出。验证、源身份变化或目标冲突导致失败时保留草稿及窗口，可修正外部条件后重试；源包和已有目标均保留，输出路径继续遵守下述新目录约束

## 只读 Replay 工作台

```sh
cargo run --locked -p cocobeat-lab -- workbench-replay PACKAGE REPLAY.json --locale zh-CN
```

`workbench-replay PACKAGE REPLAY [--locale CODE]` 共用源包完整验证和 Replay 身份检查，复用同一窗口、波形、语言、主控与整数光标；工具栏只提供缩放和返回，隐藏帧编辑与导出，快捷键和拖动也不会修改 Anchor、撤销历史或源文件

P1 青色、P2 橙色标记显示原始 Hit，Anchor 位于波形下方；列表先列录制顺序的 facts，再列 core 确认顺序的 events，最多绘制 16 行并随选择滚动，同帧记录可逐条选择。选择判定或配对时定位精确帧并高亮关联 Hit，详情顶部显示选中记录，随后是来源、规则与最终摘要

Tab / 肩键在工具栏、时间线、列表和详情间切换焦点；列表内方向键或滚轮逐条浏览，Enter / South 查看详情；640×480 窗口通过列表 / 详情页切换保留空间，详情支持换行和方向键 / 滚轮滚动到底。选中水位时保留负预滚或 EOF 后的原始帧值，手动光标导航仍限于歌曲范围

本入口没有试听或物理计时测量；Replay v1 没有设备时间戳和视觉版本，界面不推算物理输入延迟，也不承诺复现录制时的视觉效果。本批软件与受控 GPU 证据见 [Replay 诊断验证](replay-diagnostics.md#软件验证)

## 只读候选证据

`workbench-candidates PACKAGE REPORT [--locale CODE]` 打开已验证包的 Anchor 提案报告，复用明确采用入口的完整版本、来源、策略与重新编译核对；篡改、错误来源或不支持的报告均拒绝

候选以原始零基 onset index 逐项浏览，波形分开显示候选和原谱面；详情首先显示选中证据、提案和阻挡候选，再列相邻 beat、所在 section、energy 与来源信息。未知置信度保持 `null`，实验策略选择不等于生产准入

密集同像素候选仍可逐项选择；列表与详情使用现有主控、焦点和滚动规则，640×480 窗口可查看详情到底。此模式只提供缩放和返回，不修改 Anchor、历史、报告或包，明确采用继续使用 `adopt-anchor-proposal`

[观察记录](../testdata/synthetic/candidates-workbench-observations-20261007.json)保留 28 项软件测试、三组原生 GPU 窗口与 11 张目检 PNG，以及修正前的实际失败；构造候选仅验证证据查看和只读行为，不证明 MIR 质量、试听或物理输入

## 使用方式

从仓库根目录运行，先验证源包并取得 `Package BLAKE3`，把完整 64 个小写十六进制字符加上 `package-blake3:` 前缀，填入补丁的 `source_content_id`

```sh
cargo run --locked -p cocobeat-lab -- verify-package /path/to/song-package
cargo run --locked -p cocobeat-lab -- edit-anchors /path/to/song-package /path/to/patch.json /path/to/edited-package
cargo run --locked -p cocobeat-game -- --package /path/to/edited-package
```

输出目录的父目录必须存在，输出目录必须尚不存在且位于源包外；同名文件、空目录、非空目录和符号链接均会被拒绝，不能把新包导出到源包内部，包括通过父目录别名指向源包的路径

成功后 stdout 输出一行 JSON，包含 `source_content_id`、新 `content_id`、脚本 `operations` 数、`anchor_count`、`end_frames` 与身份是否改变的 `changed`；失败返回非零退出码，执行某条操作失败时错误包含从 1 开始的操作序号，所有操作成功后才进入包导出

## 补丁格式

补丁是 UTF-8 JSON，`schema_version` 固定为 `1`，完整文件最多 1 MiB，`operations` 最多 1024 项，顶层与操作内的未知字段均拒绝；源身份必须与已验证包的完整 manifest 身份一致，不能用歌曲名称或仅音频哈希代替

下面的例子适用于 [SongPackage 文档](song-package.md#手工-authoring-json) 中的一秒示例包，执行前须把来源身份占位符换成实际值

```json
{
  "schema_version": 1,
  "source_content_id": "package-blake3:<完整64个小写十六进制字符的包哈希>",
  "operations": [
    { "op": "move", "id": 1, "frame": 12400 },
    { "op": "add", "id": 3, "frame": 24000 },
    { "op": "remove", "id": 2 },
    { "op": "undo" },
    { "op": "redo" }
  ]
}
```

| 操作 | 字段 | 行为 |
|---|---|---|
| `add` | `id`、`frame` | 添加尚不存在的 Anchor ID |
| `remove` | `id` | 删除已有 Anchor |
| `move` | `id`、`frame` | 把已有 Anchor 移到精确帧，保持 ID |
| `undo` | 无 | 撤销最近一次有效变更 |
| `redo` | 无 | 恢复最近一次撤销的变更 |

`id` 是 `u64` 整数，`frame` 是 `i64` 整数，不接受小数或越界整数；帧从最终 48 kHz 音频的第 0 帧计算，48,000 帧为一秒，合法范围为 `0 <= frame < canonical_frames`，EOF 本身不能放 Anchor

Anchor ID 唯一，最多 100,000 项，列表保持 `(frame, id)` 排序；同一帧可以有多个不同 ID，不自动量化、补谱、限制音乐密度或调整 SectionCue，合法帧与排序不代表落点已通过音乐合理性判断

编辑核心只保存每次变更前后的单个 Anchor，最多保留 1024 次变更的 undo / redo 历史；新变更清空 redo，移动到原帧不增加历史也不清空 redo，空历史操作和历史已满时的新变更报错，失败操作不改变列表或历史

## 原字节保留与内容身份

导出始终保留源 `song.audio.ogg` 与 `analysis.bin` 的原始字节，不转码或重新计算能量；chart 的 SectionCue、规则和音频引用保持原值，仅替换 Anchor

最终 Anchor 与源包相同时，包括空操作或全部撤销回原态，四个对象均保留原字节、版本与包身份；合法但非规范编码的元数据也直接保留，不因重新编码制造内容变化

Anchor 实际改变时，重新编码 `chart.bin`，将 `chart_version` 设为 `manual-editor-v1`，重算 chart 的长度 / BLAKE3 和完整 manifest 的 `package_hash`；歌曲 ID、导入器版本、分析版本及其他未编辑字段保持原值，相同源包和最终 Anchor 得到相同导出字节与身份

导出先验证来源及其身份，再在目标同父目录创建专用 staging；复制的实际音频和分析字节必须符合源引用，无变化时保留本次验证使用的 manifest 原字节，完整新包通过验证后才发布；失败只清理本次创建的文件及空 staging，保留源包和已有输出，详细发布边界见 [SongPackage 的构建与清理契约](song-package.md#构建发布与失败清理)

原 Replay 绑定原包的完整内容身份，真实编辑后的新包会拒绝原 Replay，不能只因音频相同就沿用；无变化导出保留身份，仍可使用原 Replay，现有验证入口为 `cocobeat-game --package DIR --replay FILE`，此命令不提供改绑或改谱后试算

## 实现边界

[cocobeat-editor](../crates/cocobeat-editor/src/lib.rs) 只依赖 schema / std，负责精确编辑与有界历史；[lab/editor.rs](../tools/cocobeat-lab/src/editor.rs) 负责补丁、CLI 与来源身份检查，[lab/workbench.rs](../tools/cocobeat-lab/src/workbench.rs) 组合窗口、输入、波形与后台保存，[media::export_anchors](../crates/cocobeat-media/src/package.rs) 负责四对象保真导出和包身份，core 的判定规则、Replay v1 和游戏音频生命周期保持原契约
