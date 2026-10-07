# ONNX Runtime and corresponding Eigen source

The bundled SDK is the official ONNX Runtime CPU 1.30.0 build at commit `f2c39fe2f838cf35ce7da92824f5a5e3ee6e88a7`; its complete original LICENSE, ThirdPartyNotices.txt, VERSION_NUMBER and GIT_COMMIT_ID accompany each selected SDK under `licenses/onnxruntime/sdk`

Exact ONNX Runtime source is available at [this commit](https://github.com/microsoft/onnxruntime/tree/f2c39fe2f838cf35ce7da92824f5a5e3ee6e88a7) and [source archive](https://github.com/microsoft/onnxruntime/archive/f2c39fe2f838cf35ce7da92824f5a5e3ee6e88a7.tar.gz), rather than a moving main branch

Eigen Covered Software uses MPL-2.0; corresponding source is available at [Eigen commit 1d8b82b0740839c0de7f1242a3585e3390ff5f33](https://github.com/eigen-mirror/eigen/tree/1d8b82b0740839c0de7f1242a3585e3390ff5f33) and [the exact source archive](https://github.com/eigen-mirror/eigen/archive/1d8b82b0740839c0de7f1242a3585e3390ff5f33/eigen-1d8b82b0740839c0de7f1242a3585e3390ff5f33.zip), declared by ORT deps.txt with upstream SHA-1 `05b19b49e6fbb91246be711d801160528c135e34`

ORT applies [s390x-build.patch](https://github.com/microsoft/onnxruntime/blob/f2c39fe2f838cf35ce7da92824f5a5e3ee6e88a7/patches/eigen/s390x-build.patch) and [s390x-build-werror.patch](https://github.com/microsoft/onnxruntime/blob/f2c39fe2f838cf35ce7da92824f5a5e3ee6e88a7/patches/eigen/s390x-build-werror.patch) through [eigen.cmake](https://github.com/microsoft/onnxruntime/blob/f2c39fe2f838cf35ce7da92824f5a5e3ee6e88a7/cmake/external/eigen.cmake); exact retained copies, dependency recipe and patch originals are included in `eigen-patches`, with no project modifications to these materials

Recipients retain source modification rights under [MPL 2.0 sections 3.1–3.3](https://www.mozilla.org/en-US/MPL/2.0/); project source is available from the CoCoBeat repository at the commit in BUILD-INFO.txt under the included project LICENSE

The SDK full notices describe a superset of optional providers, test components and language bindings; preserving them does not claim every listed component is linked into every CPU library. The inspected SDK notices select Apache-2.0 for Mbed TLS and contain no identified GPL-only SDK declaration; this does not label the entire native or system dependency closure MIT

Linux SDKs dynamically use system glibc libraries (LGPL-2.1-or-later) and libstdc++ / libgcc_s (GPL-3.0-or-later WITH GCC Runtime Library Exception), not bundled copies. Preserve users' library replacement and debugging rights; [glibc license](https://sourceware.org/glibc/manual/latest/html_node/Copying.html) and [GCC Runtime Library Exception](https://gcc.gnu.org/onlinedocs/libstdc++/manual/license.html) apply to those system libraries. Adding bundled or static system runtimes requires a fresh source / notice / relinking review

Windows SDK archives omit the required VC runtime DLLs. Install the architecture-appropriate official supported [Microsoft Visual C++ Redistributable](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist) before running native analysis; do not copy runtime DLLs from a developer's machine. Native Windows load/Run and clean-machine prerequisite validation require their own software receipts
