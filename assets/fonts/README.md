# 界面字体

使用官方 Noto Sans 系列，六个字体共 30,929,960 bytes（29.50 MiB），字体和许可原字节保持不变

| 界面语言 | 文件 | 上游 family | Unicode 映射数 |
|---|---|---|---|
| en-US、en-GB、es-419、pt-BR、fr、de、ru、uk | `NotoSans[wdth,wght].ttf` | Noto Sans | 3,094 |
| 简体中文 | `NotoSansSC-Regular.otf` | Noto Sans SC | 30,890 |
| 繁體中文（台灣） | `NotoSansTC-Regular.otf` | Noto Sans TC | 20,745 |
| 繁體中文（香港） | `NotoSansHK-Regular.otf` | Noto Sans HK | 20,755 |
| 日本語 | `NotoSansJP-Regular.otf` | Noto Sans JP | 16,732 |
| 한국어 | `NotoSansKR-Regular.otf` | Noto Sans KR | 23,174 |

基础 Noto Sans 2.015 来自 [Google Fonts 官方目录](https://github.com/google/fonts/tree/8b0a1d0f5983c89bc2b93f1b5fb55f9e252744b5/ofl/notosans)，可变 TrueType 字体的默认轴为 `wght=400`、`wdth=100`，保留完整上游拉丁、希腊与西里尔字符集；CJK 取 [Noto Sans CJK 2.004](https://github.com/notofonts/noto-cjk/releases/tag/Sans2.004) 官方地区字集的五个 Regular OTF，繁中台湾与香港各使用本地区字形

保留完整上游地区字集以容纳动态文字，不按当前菜单裁切；语言选择器每项的自称名称使用对应语言字体，英语两项分别为 `English (United States)`、`English (United Kingdom)`，不能让整个语言名单共用单个地区字体或依赖系统字体补字

Bevy 现有 `FontLoader` 支持 `.ttf` 与 `.otf`，或用 `Font::from_bytes` 加载嵌入字节；来源 URL、版本、固定提交、Git blob、SHA-256、内嵌版权和语言映射见 [SOURCES.json](SOURCES.json)

发行包随字体分别提供两份完整上游许可：[NotoSans-OFL.txt](NotoSans-OFL.txt) 与 [NotoSansCJK-OFL.txt](NotoSansCJK-OFL.txt)，均为 SIL Open Font License 1.1；基础字体的版权为 Noto Project Authors，官方 CJK 字体内嵌的 Adobe 2014–2021 版权同样保留并逐文件记入来源清单，许可文件未合并或改写

五个 CJK 字体内的原始声明为 `© 2014-2021 Adobe (http://www.adobe.com/).` 与 `Noto is a trademark of Google Inc.`，基础字体的声明为 `Copyright 2022 The Noto Project Authors (https://github.com/notofonts/latin-greek-cyrillic)`；将字体嵌入程序时也随发行保留本说明、来源清单与两份许可

资源检查使用开发环境已安装的 fontTools，不属于游戏运行时依赖

```sh
python3 assets/fonts/verify.py
python3 assets/fonts/verify.py --text uk /tmp/cocobeat-ui-uk.txt
```

默认检查原字节与官方 Git blob 一致、字体 family、版本、默认轴、13 种语言的自称及字符样例，涵盖拉丁重音字母、俄文与乌克兰文字母、香港常用字和全部 11,172 个现代韩文音节；`--text` 按指定语言额外检查 UTF-8 文案的 Unicode 映射，只忽略空白及控制字符

资源检查不代替 13 种语言真实文案的完整性、字体栅格化、布局和导航验收；地区字集不保证覆盖任意 Unicode 字符，新增动态文字仍需按实际内容处理缺字
