# Beat This! CPU candidate probe

研究工具，尚未接入产品；[实际结果摘要](results-2026-10-07.json)记录数值 PASS、质量缺口和 NOT_RUN 项，原 MIR 失败基准保持不变

## 依赖准备

本机 Python 3.14，使用项目内 venv 与 cache；以下为已获授权并完成安装的可复现命令，CPU wheel 来源显式固定，其他包解析当前兼容新版，安装后的完整版本留在验证证据中

```bash
beat_repo="$PWD"
beat_work="$beat_repo/target/mir-model-20261007"
mkdir -p "$beat_work"
cd "$beat_work"
export UV_CACHE_DIR="$beat_work/uv-cache"
uv venv --python /usr/bin/python3 "$beat_work/.venv"
uv pip install --python "$beat_work/.venv/bin/python" --only-binary=:all: \
  'torch @ https://download.pytorch.org/whl/cpu/torch-2.14.1%2Bcpu-cp314-cp314-manylinux_2_28_x86_64.whl#sha256=35103e180214793207f95d7e12e100ecba3ace6f5fb37e1fcefbb07e096ed180' \
  'torchaudio @ https://download.pytorch.org/whl/cpu/torchaudio-2.11.0%2Bcpu-cp314-cp314-manylinux_2_28_x86_64.whl#sha256=34c5dcd704e17a2b01c097b4fe3b5f83c5cdbc42b9f2abd095e026c588f873d4' \
  beat-this onnx onnxruntime
uv pip freeze --python "$beat_work/.venv/bin/python" > "$beat_work/environment.txt"
```

核对时间为 2026-10-07：CPU torch 196,260,555 B、CPU torchaudio 341,339 B、ONNX 约 8.9 MB、ORT Python wheel 23.6 MB、NumPy 16.7 MB、small0 权重 8,451,101 B；预计下载约 280 MB，上限预算 350 MB，安装和证据预留 2 GiB，推理线程为 2

CPU wheel 元数据不包含 CUDA、NVIDIA 或 Triton；[TorchAudio 官方说明](https://github.com/pytorch/audio)确认 2.11 兼容 torch 2.11 及以后的版本；基础模型对照不需要 Lightning、madmom、训练集、ffmpeg 或额外解码器

所有 probe 在研究目录运行；ORT 1.30.0 导入在 sandbox 下会产生 `:memory:.ses` 会话副产物，已通过隔离 import 重现，与[上游记录](https://github.com/microsoft/onnxruntime/issues/32476)一致；调用禁用 telemetry API 不阻止导入期文件创建

## 模型与转换

[Beat This! 官方仓库](https://github.com/CPJKU/beat_this)声明源码和发布权重为 MIT；将[官方 small0](https://cloud.cp.jku.at/public.php/dav/files/7ik4RrBKTS273gp/small0.ckpt)保存到研究目录，转换时必须传入存在的本地文件和已核验 SHA-256，不使用会自动下载的短名称

```bash
"$beat_work/.venv/bin/python" -B "$beat_repo/tools/beat-model-probe/probe.py" matrix "$beat_work/source-matrix"
"$beat_work/.venv/bin/python" -B "$beat_repo/tools/beat-model-probe/probe.py" export \
  --checkpoint "$beat_work/small0.ckpt" \
  --checkpoint-sha256 6074be2c4d490c5f6101fcc374a1ec72ae93456e23bb6019783b849f5dc7d47b \
  --output "$beat_work/small0.onnx"
```

导出使用 opset 17、float32 和动态时间维，比较 128、257、1499、1500 帧的官方和 ORT 输出，预先固定 `atol=1e-4, rtol=1e-4`；当前 Linux x64 CPU 实测八项数值对照通过，具体身份和误差保存在研究目录 `small0.export.json`，不外推到其他批量和任意时间长度

## 矩阵与证据边界

矩阵包含固定 BPM、非整数 BPM、渐变速度、3/4、6/8、弱起、摇摆、静默、反相和单边声道，全部为独立生成的 CC0 合成输入；6/8 的 beat 参考单位明确为附点四分音符，不把八分音符脉冲误标成 quarter beat

`matrix` 输出 WAV、同一量化信号的 float32 stereo PCM 和显式事件参考；`canonicalize` 先核对 WAV 格式及样本与 manifest PCM 完全一致，再调用实际媒体 driver 编码和严格回读，核对两次返回元数据，固定 manifest/driver/源文件及中间 OGG 的 SHA-256；任何漂移或不匹配都不发布 `canonical_strict_readback` 矩阵，driver 对应的构建身份仍需另外记录

```bash
"$beat_work/.venv/bin/python" -B "$beat_repo/tools/beat-model-probe/probe.py" canonicalize \
  --matrix "$beat_work/source-matrix/matrix.json" \
  --driver "$beat_repo/target/vorbis-admission-20261007/shipping-build/bin/native-vorbis-check" \
  --destination "$beat_work/canonical-matrix"
"$beat_work/.venv/bin/python" -B "$beat_repo/tools/beat-model-probe/probe.py" compare \
  --checkpoint "$beat_work/small0.ckpt" \
  --checkpoint-sha256 6074be2c4d490c5f6101fcc374a1ec72ae93456e23bb6019783b849f5dc7d47b \
  --onnx "$beat_work/small0.onnx" \
  --matrix "$beat_work/canonical-matrix/matrix.json" \
  --output "$beat_work/comparison.json"
```

开发期源信号 smoke 可显式使用 `--allow-source-smoke`，其来源保留在每行证据中；官方预处理逐声道调用，避免 stereo 平均消掉反相信号，谱图随后同时交给官方 PyTorch 和 ORT，复用官方 1500 帧切片、6 帧边界和 minimal postprocessor

数值一致性和算法质量分开：数值需满足预定容差及完全相同的后处理坐标；质量仅报告按 70 ms 窗口一对一匹配的 TP/FP/FN/F1，没有自动准入结论，静默输出按 FP 计；20 ms 模型帧步长与相邻峰平均所得坐标均不构成精确 onset 证据，置信度校准保持未知

源矩阵实际发现官方坐标可等于 PCM 总时长，报告显式记录越界点；官方反射 padding 也拒绝极短输入，生产接入需要明确 `[0, frames)` 和短输入不足状态，不能默认补拍或把异常变成空结果成功

当前比较复用 Python 官方前处理，只证明同谱图下的模型导出和推理一致性；生产 48 kHz→22.05 kHz 重采样、STFT/mel 前处理、C API 原生接入和四平台实跑仍需单独证明，原 MIR 失败基准保持不变

## 原生 ORT 四目标

[ONNX Runtime v1.30.0 官方发行](https://github.com/microsoft/onnxruntime/releases/tag/v1.30.0)提供四个 CPU 包，Linux x64/aarch64 分别为 11,306,877 / 10,269,495 B，Windows x64/ARM64 分别为 82,645,522 / 83,954,906 B；精确 URL 和 GitHub 发布的 SHA-256 记录在研究 evidence 中

ORT 为 [MIT](https://github.com/microsoft/onnxruntime/blob/v1.30.0/LICENSE)，分发还要保留对应发行包的 LICENSE、ThirdPartyNotices 和依赖 notice；MPL 工程可将 ORT 动态库及模型随发行包附带，通过 C API 在进程内执行，无需运行时 Python、下载或子进程；以上是发行来源与接入路线，四平台依赖解析、运行及数值一致性尚未通过

## BSD DBN 窄对照

`dbn_probe.py` 仅用于研究：从固定 [madmom 官方提交](https://github.com/CPJKU/madmom/tree/27f032e8947204902c675e5e341a3faf5dc86dae)提取 BSD 源码，保留节点原文及范围，编译单个 `hmm` 扩展，不安装 madmom 整包、不取得或加载非商用模型；原始文件、许可、SHA-256、源码提取记录和兼容 diff 都保存在研究目录

开发期先在同一 venv 安装 `scipy Cython`，按结果 JSON 的 `madmom_source.files` 下载原始文件到 `dbn-source`，并将 `madmom_source` 对象保存为其中的 `manifest.json`；`source` 命令逐文件核验 SHA-256 后抽取代码

新增 wheel 合计 38,783,864 B，不需要 mido；唯一兼容改动把 `numpy.math` 的 `INFINITY` 改为 `libc.math`，对应[上游 PR #548](https://github.com/CPJKU/madmom/pull/548)，不修改 DBN 运算或阈值

```bash
"$beat_work/.venv/bin/python" -B "$beat_repo/tools/beat-model-probe/dbn_probe.py" source \
  --source "$beat_work/dbn-source" \
  --package "$beat_work/dbn-package"
(cd "$beat_work/dbn-package" && "$beat_work/.venv/bin/python" -B setup.py build_ext --inplace --parallel 2)
"$beat_work/.venv/bin/python" -B "$beat_repo/tools/beat-model-probe/dbn_probe.py" compare \
  --package "$beat_work/dbn-package" \
  --checkpoint "$beat_work/small0.ckpt" \
  --checkpoint-sha256 6074be2c4d490c5f6101fcc374a1ec72ae93456e23bb6019783b849f5dc7d47b \
  --onnx "$beat_work/small0.onnx" \
  --matrix "$beat_work/canonical-matrix/matrix.json" \
  --output "$beat_work/dbn-canonical-comparison.json"
```

比较同时保留原 minimal、官方 `[3,4]` 和独立实验 `[2,3,4]` 三组；直接复用 Beat This! 官方 sigmoid 与互斥 beat/downbeat 概率变换，输出原始坐标、越界点和 70 ms F1，不裁剪或改变参考拍单位来掩盖失败；DBN 从候选拍数中选择最大路径概率不等于可靠拍号判定，20 ms 网格仍不能承担精确 onset

源 PCM 与修补后真实 Vorbis 严格回读分别完成 120 行三组数值对照，全部通过；后者固定 driver SHA `199953f3…` 与 301 项构建输入，最大 logit 误差 5.63e-5；官方 DBN 改善部分强拍，但变速、6/8 和摇摆退步，扩展 `[2,3,4]` 未解决，不能作为可靠拍号或完整分析准入

DBN 的回溯内存随帧数和状态数增长；55–215 BPM / 50 fps 下四拍模型有 5,796 个状态，十分钟输入仅回溯指针约 663 MiB，研究输入限制 120 秒；Python 源码对照不证明原生四目标生产接入
