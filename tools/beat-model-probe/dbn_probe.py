#!/usr/bin/env python3
"""Research-only BSD madmom DBN subset; never install or load madmom models"""

import argparse
import ast
import difflib
import json
import os
from pathlib import Path
import shutil
import sys

from probe import compare as compare_model
from probe import digest, write_json


def build_source(args):
    manifest = json.loads((args.source / "manifest.json").read_text())
    for entry in manifest["files"]:
        if digest(args.source / entry["path"]) != entry["sha256"]:
            raise ValueError("official source identity changed")
    args.package.mkdir(parents=True, exist_ok=False)
    extracted = []
    for name in ("madmom", "madmom/ml", "madmom/features"):
        folder = args.package / name
        folder.mkdir(exist_ok=True)
        (folder / "__init__.py").write_text("")
    shutil.copyfile(args.source / "LICENSE", args.package / "LICENSE")
    shutil.copyfile(args.source / "madmom/features/beats_hmm.py",
                    args.package / "madmom/features/beats_hmm.py")

    def select(filename, names, imports=""):
        path = args.source / "madmom" / filename
        source = path.read_text()
        lines = source.splitlines(keepends=True)
        nodes = [node for node in ast.parse(source).body if getattr(node, "name", None) in names]
        assert {node.name for node in nodes} == set(names)
        selected = []
        for node in nodes:
            selected.append("".join(lines[node.lineno - 1:node.end_lineno]))
            extracted.append({"path": f"madmom/{filename}", "symbol": node.name,
                              "start_line": node.lineno, "end_line": node.end_lineno})
        (args.package / "madmom" / filename).write_text(
            "# BSD source excerpt; full copyright and terms in the package LICENSE\n" + imports + "\n\n" + "\n\n".join(selected) + "\n"
        )

    select("processors.py", ["Processor"])
    select("features/beats.py", ["threshold_activations"], "import numpy as np")
    select("features/downbeats.py", ["_process_dbn", "DBNDownBeatTrackingProcessor"],
           "import numpy as np\nfrom .beats import threshold_activations\n"
           "from .beats_hmm import BarStateSpace, BarTransitionModel, RNNDownBeatTrackingObservationModel\n"
           "from ..ml.hmm import HiddenMarkovModel\nfrom ..processors import Processor")
    original = (args.source / "madmom/ml/hmm.pyx").read_text()
    before = "from numpy.math cimport INFINITY"
    assert original.count(before) == 1
    patched = original.replace(before, "from libc.math cimport INFINITY")
    (args.package / "madmom/ml/hmm.pyx").write_text(patched)
    (args.package / "numpy-math.patch").write_text("".join(difflib.unified_diff(
        original.splitlines(keepends=True), patched.splitlines(keepends=True),
        fromfile="official/madmom/ml/hmm.pyx", tofile="subset/madmom/ml/hmm.pyx")))
    (args.package / "setup.py").write_text(
        "from setuptools import Extension, setup\nfrom Cython.Build import cythonize\nimport numpy as np\n"
        "setup(name='cocobeat-dbn-code-probe', ext_modules=cythonize([Extension('madmom.ml.hmm', "
        "['madmom/ml/hmm.pyx'], include_dirs=[np.get_include()])], compiler_directives={'language_level': 3}))\n"
    )
    write_json(args.package / "subset.json", {
        "upstream_commit": manifest["commit"], "source_manifest_sha256": digest(args.source / "manifest.json"),
        "script_sha256": digest(__file__), "exact_source_excerpts": extracted,
        "omitted": "package eager imports, audio/nn/CRF/comb extensions and all models; no DBN algorithm changes",
        "compatibility_patch": "numpy.math INFINITY -> libc.math INFINITY only",
        "patch_source": "https://github.com/CPJKU/madmom/pull/548/commits/868e004eb3e36f4359d11458e5849f9d7e52b644",
        "models_downloaded_or_installed": False,
    })


def compare(args):
    sys.path.insert(0, str(args.package.resolve(strict=True)))
    from beat_this.model.postprocessor import Postprocessor
    from madmom.features.downbeats import DBNDownBeatTrackingProcessor

    def block_external_models(event, values):
        if event == "socket.connect":
            raise RuntimeError("research inference cannot access the network")
        if event == "open" and isinstance(values[0], (str, bytes)):
            path = os.fsdecode(values[0]).replace("\\", "/")
            if "madmom/models" in path or path.endswith((".pkl", ".pickle")):
                raise RuntimeError("madmom model access forbidden")
    sys.addaudithook(block_external_models)
    default = Postprocessor(type="dbn", fps=50)
    extended = Postprocessor(type="dbn", fps=50)
    extended.dbn = DBNDownBeatTrackingProcessor(
        beats_per_bar=[2, 3, 4], min_bpm=55.0, max_bpm=215.0,
        fps=50, transition_lambda=100, num_threads=1)
    processors = {"minimal": Postprocessor(type="minimal", fps=50),
                  "official_dbn_3_4": default, "experimental_dbn_2_3_4": extended}
    compare_model(args, processors, {
        "script_sha256": digest(__file__),
        "subset": json.loads((args.package / "subset.json").read_text()),
        "hmm_extension_sha256": digest(next((args.package / "madmom/ml").glob("hmm.*.so"))),
        "loaded_madmom_modules": sorted(name for name in sys.modules if name.startswith("madmom")),
        "state_counts": {str(beats): int(hmm.transition_model.num_states)
                         for beats, hmm in zip(extended.dbn.beats_per_bar, extended.dbn.hmms)},
    })
    if any(name == "madmom.models" or name.startswith("madmom.models.") for name in sys.modules):
        raise RuntimeError("madmom models imported")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    build = commands.add_parser("source")
    build.add_argument("--source", type=Path, required=True)
    build.add_argument("--package", type=Path, required=True)
    compare_parser = commands.add_parser("compare")
    for option in ("package", "checkpoint", "onnx", "matrix", "output"):
        compare_parser.add_argument(f"--{option}", type=Path, required=True)
    compare_parser.add_argument("--checkpoint-sha256", required=True)
    compare_parser.add_argument("--allow-source-smoke", action="store_true")
    args = parser.parse_args()
    for variable in ("OMP_NUM_THREADS", "MKL_NUM_THREADS", "OPENBLAS_NUM_THREADS", "RAYON_NUM_THREADS"):
        os.environ[variable] = "2"
    {"source": build_source, "compare": compare}[args.command](args)


if __name__ == "__main__":
    main()
