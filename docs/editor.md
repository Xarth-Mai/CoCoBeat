# Anchor 命令行编辑

`edit-anchors PACKAGE PATCH NEW_PACKAGE` 用整数音频帧修正已有 SongPackage 的 Anchor，并导出可重新加载的新包；当前支持增删、移动、撤销和重做，不修改分析事实、SectionCue 或判定规则，包格式继续使用 [SongPackage v1](song-package.md)

这是可保存补丁、可重复执行的 CLI 编辑入口，时间线 UI、波形 GUI、试听校准、候选证据视图和正式菜单接入仍未实现；候选报告与明确采用见 [Anchor 提案](anchors.md)，原始输入、判定与配对的 JSONL 查看入口见 [Replay 诊断](replay-diagnostics.md)

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

[cocobeat-editor](../crates/cocobeat-editor/src/lib.rs) 只依赖 schema / std，负责精确编辑与有界历史；[lab/editor.rs](../tools/cocobeat-lab/src/editor.rs) 负责补丁、CLI 与来源身份检查，[media::export_anchors](../crates/cocobeat-media/src/package.rs) 负责四对象保真导出和包身份，core 的判定规则、Replay v1、输入和音频生命周期保持原契约
