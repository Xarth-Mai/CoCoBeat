# Experimental native Beat This model

This project-exported small0 ONNX retains the original [Beat This code and weights MIT license](https://github.com/CPJKU/beat_this#license), included in `licenses/beat-this/LICENSE`; fixed provenance and export identity are in PROVENANCE.json

The explicit Lab experimental import uses the selected left or right channel from final canonical PCM; ordinary hand-authored imports and game Ready menus keep their existing behavior. Successful candidates remain Candidate / Algorithm with unknown confidence; historical frontend and music quality failures remain open and automatic Anchors are not enabled

Resources load from the executable's release root: Windows Lab at the root, Linux Lab under `bin`, SDK under `lib/onnxruntime`, and this model under `assets/models/beat-this`. Production checks use compiled sizes and BLAKE3 identities, not this editable provenance document as a trust root; products do not download or execute Python to analyze songs

Windows needs the matching official [VC++ Redistributable](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist). Linux uses the system shared runtimes described in `licenses/onnxruntime/SOURCE-AVAILABILITY.md`; the SDK's minimum ABI does not lower the game's Ubuntu 24.04 build baseline
