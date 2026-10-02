#!/usr/bin/env python3
"""Check bundled Noto font provenance and optional localized UTF-8 text coverage."""

import argparse
import hashlib
import json
from pathlib import Path
import unicodedata

from fontTools.ttLib import TTFont


SAMPLES = {
    "zh-CN": "简体中文 设置 应用 取消 恢复默认 显示 分辨率 全屏 语言 画质 保存失败",
    "en-US": "English (United States) Settings Apply Cancel Restore defaults Display Resolution",
    "en-GB": "English (United Kingdom) Settings Apply Cancel Restore defaults Display Resolution",
    "ja": "日本語 設定 適用 キャンセル 初期設定に戻す 表示 解像度 全画面 言語 画質 保存に失敗",
    "ko": "한국어 설정 적용 취소 기본값 복원 표시 해상도 전체 화면 언어 화질 저장 실패",
    "zh-TW": "繁體中文（台灣） 設定 套用 取消 恢復預設 顯示 解析度 全螢幕 語言 畫質 儲存失敗",
    "zh-HK": "繁體中文（香港） 設定 套用 取消 還原預設 顯示 解像度 全螢幕 語言 畫質 儲存失敗 邨裏綫",
    "es-419": "Español (Latinoamérica) Configuración Aplicar Cancelar ÁÉÍÓÚÜÑ áéíóúüñ ¿¡",
    "pt-BR": "Português (Brasil) Configurações Aplicar Cancelar ÁÀÂÃÉÊÍÓÔÕÚÇ áàâãéêíóôõúç",
    "fr": "Français Paramètres Appliquer Annuler ÀÂÆÇÉÈÊËÎÏÔŒÙÛÜŸ àâæçéèêëîïôœùûüÿ",
    "de": "Deutsch Einstellungen Anwenden Abbrechen ÄÖÜẞ äöüß",
    "ru": "Русский Настройки Применить Отмена АБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯ абвгдеёжзийклмнопрстуфхцчшщъыьэюя",
    "uk": "Українська Налаштування Застосувати Скасувати АБВГҐДЕЄЖЗИІЇЙКЛМНОПРСТУФХЦЧШЩЬЮЯ абвгґдеєжзиіїйклмнопрстуфхцчшщьюя",
}


def missing(text, cmap):
    return sorted(
        {
            f"U+{ord(char):04X} {char}"
            for char in text
            if not char.isspace()
            and unicodedata.category(char) != "Cc"
            and ord(char) not in cmap
        }
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--text", nargs=2, metavar=("LOCALE", "UTF8_FILE"))
    args = parser.parse_args()
    if args.text and args.text[0] not in SAMPLES:
        parser.error("LOCALE must be one of: " + ", ".join(SAMPLES))
    root = Path(__file__).resolve().parent
    sources = json.loads((root / "SOURCES.json").read_text(encoding="utf-8"))
    assert sources["locales"] == list(SAMPLES)
    assert set(SAMPLES) == {
        locale for row in sources["files"] for locale in row.get("locales", [])
    }
    report = []
    for row in sources["files"]:
        path = root / row["file"]
        data = path.read_bytes()
        assert len(data) == row["bytes"], f"size mismatch: {path.name}"
        assert hashlib.sha256(data).hexdigest() == row["sha256"], path.name
        blob = b"blob " + str(len(data)).encode() + b"\0" + data
        assert hashlib.sha1(blob).hexdigest() == row["git_blob_sha1"], path.name
        if "locales" not in row:
            continue
        with TTFont(path) as font:
            cmap = font.getBestCmap()
            assert font["name"].getDebugName(1) == row["family"]
            assert font["name"].getDebugName(5) == row["font_version"]
            assert font["name"].getDebugName(0) == row["copyright"]
            assert len(cmap) == row["unicode_mappings"]
            if "default_axes" in row:
                assert {a.axisTag: a.defaultValue for a in font["fvar"].axes} == row["default_axes"]
            text = "".join(chr(code) for code in range(32, 127)) + "×—–…"
            text += " ".join(SAMPLES[locale] for locale in row["locales"])
            if "ko" in row["locales"]:
                text += "".join(chr(code) for code in range(0xAC00, 0xD7A4))
            if args.text and args.text[0] in row["locales"]:
                text += Path(args.text[1]).read_text(encoding="utf-8")
            absent = missing(text, cmap)
            assert not absent, f"{path.name}: {', '.join(absent)}"
            report.append({"file": path.name, "locales": row["locales"], "status": "PASS"})
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
