# 语言选择旗帜

SVG 原件来自 [flag-icons v7.5.0](https://github.com/lipis/flag-icons/releases/tag/v7.5.0)，遵循本目录的 MIT `LICENSE`；固定 commit、原件与派生文件 SHA-256 见 `SOURCES.json`

界面使用 96×72 PNG，保留同源 4:3 SVG 便于后续导出；旗帜仅作为语言选择器图标，语言自称与 locale 标签决定实际语言

首批映射为 `zh-CN → cn`、`en-US → us`、`en-GB → gb`、`ja → jp`、`ko → kr`、`zh-TW → tw`、`zh-HK → hk`、`es-419 → mx`、`pt-BR → br`、`fr → fr`、`de → de`、`ru → ru`、`uk → ua`；墨西哥旗帜代表拉美西语选择项，语言名称保留 `Español (Latinoamérica)`

在项目根目录使用已安装的 `rsvg-convert` 重新导出，无需运行时 SVG 依赖

```sh
for source in assets/flags/*.svg; do
    rsvg-convert --width 96 --height 72 --output "${source%.svg}.png" "$source"
done
```

检查许可、SVG 与 PNG 是否仍匹配来源记录

```sh
python3 - <<'PY'
from pathlib import Path
import hashlib, json
root = Path('assets/flags')
sources = json.loads((root / 'SOURCES.json').read_text(encoding='utf-8'))
records = [sources['license_file']]
records += [entry[kind] for entry in sources['files'] for kind in ('svg', 'png')]
for record in records:
    data = (root / record['file']).read_bytes()
    assert len(data) == record['bytes'], record['file']
    assert hashlib.sha256(data).hexdigest() == record['sha256'], record['file']
print(f'PASS: {len(records)} source and derivative hashes')
PY
```
