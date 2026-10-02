#!/usr/bin/env python3
"""Bake white RGBA coverage masks from the five editable wordmark groups"""

from pathlib import Path
import struct
import subprocess
import tempfile
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parent
SVG = "{http://www.w3.org/2000/svg}"
GROUPS = {
    "co1": "logo_co1",
    "co2": "logo_co2",
    "beat": "logo_beat",
    "eyes_blue": "eyes_blue",
    "eyes_pink": "eyes_pink",
}


def main():
    destination = ROOT / "masks"
    destination.mkdir(exist_ok=True)
    ET.register_namespace("", SVG[1:-1])
    for group_id, name in GROUPS.items():
        tree = ET.parse(ROOT / "wordmark.svg")
        root = tree.getroot()
        groups = root.findall(f"{SVG}g")
        assert sum(g.get("id") == group_id for g in groups) == 1, group_id
        for group in groups:
            if group.get("id") != group_id:
                root.remove(group)
            else:
                group.set("fill", "#fff")
        with tempfile.NamedTemporaryFile(suffix=".svg") as source:
            tree.write(source.name, encoding="UTF-8", xml_declaration=True)
            subprocess.run([
                "rsvg-convert", "--width", "3360", "--height", "720",
                "--output", str(destination / f"{name}.png"), source.name,
            ], check=True)
        header = (destination / f"{name}.png").read_bytes()[:26]
        assert header[:8] == b"\x89PNG\r\n\x1a\n"
        assert struct.unpack(">II", header[16:24]) == (3360, 720)
        assert header[24:26] == b"\x08\x06", "Expected 8-bit RGBA coverage mask"


if __name__ == "__main__":
    main()
