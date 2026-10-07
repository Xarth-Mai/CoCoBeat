# 原生 Vorbis 编码路径

`cocobeat-media::encode_canonical_audio(source, new_output)` 是标准音频的唯一生产编码候选入口，使用 `vorbis_rs` 静态内嵌 aoTuV / Lancer 和 libogg；用户已允许各模块采用适合需求的成熟库，软件准入与实际接线分别交付

## 输入与发布边界

入口消费实际 High 重采样后的 48 kHz 双声道浮点帧，逐 block 检查 finite 和 `[-4, 4]`，输入最多十分钟、输出最多 512 MiB；超域明确拒绝，不归一化、裁幅、补零或修剪时间轴，合法源文件也可能不满足当前编码域

固定质量为 q10，profile 为 `aotuv-lancer-vorbis-q10-v1`；每个独立 Ogg 对象只有一个逻辑流，使用固定 serial，完整 finish、同步并关闭后，再以输入的真实帧数严格读回，核验 48 kHz / stereo、CRC、EOS、finite 和从帧 0 开始的实际长度

输出采用 `create_new`，保留已有输出和源文件；返回错误时删除本次创建的 Ogg，并保留原错误及清理错误。第三方原生分配失败或进程终止不具备 Rust 返回错误的清理保证，不能以这些窄测宣称全输入域形式安全证明

编码成功本身不发布 SongPackage 或 Ready，后续导入须从这份最终回读音频产生真实分析、经明确采用的谱面，再经过既有四对象事务和完整包验证

## 来源与修补

主 manifest 使用主版本范围，`Cargo.lock` 固定实际 `vorbis_rs 0.5.6`、Vorbis sys `0.1.6` 和 libogg sys `0.1.5`；原始归档身份、许可和补丁分别见 [许可台账](../licenses/vorbis-rs/README.md)、[Vorbis UPSTREAM](../vendor/aotuv_lancer_vorbis_sys/UPSTREAM.md) 和 [libogg UPSTREAM](../vendor/ogg_next_sys/UPSTREAM.md)

实际 Sanitizer 先后发现初始化负值移位、Lancer 未对齐写入、libogg 读位移位和共享 codebook 提示码字移位；原错误及中间修补结果全部保留，回移三个 Xiph Vorbis 官方修复、一个完整 libogg 官方修复，并删除 Lancer 重复位打包实现，恢复已链接的原生 libogg 调用

修补保留 aoTuV 算法及质量参数，不关闭诊断；升级时核对上游是否已包含这些修补，再更新本地副本和对应证据，不仅按版本号移除补丁

## 软件验证方法

本机修补后 11 项 Clang `address,undefined,float-cast-overflow` 控制通过，包括短文件、首尾脉冲、近满幅、反相、幅度边界、超域拒绝和十分钟原创音乐；9 个有效输出完整双读回，且与初始普通矩阵的对应 Ogg 字节一致。Rust 本体未 instrument，LeakSanitizer 因 sandbox 的 ptrace 限制为 NOT RUN

[普通质量工具](../tools/native-vorbis-check/README.md) 对原始及新配方分别核验来源身份，完整比较 Symphonia 与 libvorbisfile 的同位置 PCM，不平移、增益拟合、截断或补零；音调控制预设 SNR 至少 35 dB，音乐、瞬态和边界的局部损失另外保留，不以整体指标替代听感

FFmpeg 仅作第三路开发期互操作检查，按其完整输出长度记录 PASS_COMPLETE / FAIL_COMPLETE；短文件实际长度失败保持原样，不能据另两路通过把 FFmpeg 也写成通过，产品不依赖 FFmpeg 或运行时备用编码器

四目标 [Native media candidate](../.github/workflows/media-candidate.yml) 在 Windows / Linux × x86-64 / ARM64 原生执行 media 测试和优化后的 14 项便携完整编码 / 双回读控制，保留日志、输出身份与失败；实际运行以对应源码 revision 和 Actions 记录为准，工作流定义本身不是跨平台 PASS

真实性能、seek、曲库音乐、loopback 和真人听感按各自观察记录验收；本机矩阵、Sanitizer、原生 CI 与设备 / 真人结论分别记录
