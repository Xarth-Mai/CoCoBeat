# Labels 输入组件原始许可

本目录保留独立 Labels 工作台新增的 bevy_picking 与 bevy_ui_widgets 0.19.1 原始 MIT 和 Apache-2.0 许可文本，来源为 Cargo.lock 对应的官方 registry crate 归档

两份原归档的完整 SHA-256 与 lock checksum 一致，四份许可文件与归档成员及 registry 解包文本逐字节一致，项目 MPL-2.0 不替代上游许可

| 包与当前锁定版本 | 原始许可文件 |
|---|---|
| bevy_picking 0.19.1 | [LICENSE-MIT](bevy_picking-0.19.1/LICENSE-MIT) / [LICENSE-APACHE](bevy_picking-0.19.1/LICENSE-APACHE) |
| bevy_ui_widgets 0.19.1 | [LICENSE-MIT](bevy_ui_widgets-0.19.1/LICENSE-MIT) / [LICENSE-APACHE](bevy_ui_widgets-0.19.1/LICENSE-APACHE) |

Lab 私有依赖使用 `0` 系列范围并关闭这两个包的默认 features，当前 lock 固定 0.19.1；工作台复用 Bevy 的 input_focus、EditableTextInput 和 Pointer Release 类型，游戏 runtime 不新增直接依赖

实际完整及四目标 offline/locked metadata 中，这两个包的 features 均为空；已存在的 bevy_input_focus features 为 bevy_reflect、default、gamepad、keyboard、mouse、std，解析配置见 [第三方总台账](../THIRD_PARTY.md)

本目录的原始许可随现有 licenses 目录交付，metadata 仅证明依赖解析和许可文本身份，最终编译、链接组件及四平台原生 Labels 交付分别验证
