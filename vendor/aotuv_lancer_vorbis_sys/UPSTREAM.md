# aoTuV / Lancer sys 来源与修补

从 crates.io `aotuv_lancer_vorbis_sys 0.1.6` 官方归档完整提取，原始 243 个文件及 SHA-256 见 [UPSTREAM.json](UPSTREAM.json)，归档 SHA-256 为 `5bc4fd1a61860d2f1198b60bedd30910eaffa978f1ee6214dfb24ac70d589225`；原代码、编译脚本、绑定及许可注释保留，新增顶层 LICENSE 从官方绑定项目固定版本补齐，来源见 [许可台账](../../licenses/vorbis-rs/README.md)

CoCoBeat 回移三项官方 Xiph 修复并恢复原生 libogg 调用，唯一源码差异见 [cocobeat-backport.patch](cocobeat-backport.patch)

- [2d79800b](https://github.com/xiph/vorbis/commit/2d79800b6751dddd4b8b4ad50832faa5ae2a00d9)：`psy.c` 和 `floor1.c` 的负数左移改为算术等价乘法，保留符号与数值，不关闭检测器
- [315da9cc](https://github.com/xiph/vorbis/commit/315da9cc9d30484c802b2e2ea150df39e060e2b9)：将插值基础索引限制至 `P_BANDS-2`，在计算 `del` 前保护 `noiseoff` 及 aoTuV 共享此索引的 `ntfix_offset`，避免零权重项仍发生越界求值

- [bb4047de](https://github.com/xiph/vorbis/commit/bb4047de4c05712bf1fd49b9584c360b8e4e0adf)：`sharedbook.c` 生成解码提示 codeword 时，移位前将非负索引转换为 `ogg_uint32_t`，保留完整 32 位码字，消除有符号整数溢出的未定义行为

- 本地可移植性修补：删除 Lancer 在 `codebook.c` 内的 `write/look/adv` 副本，恢复 [Xiph 原生 libogg 调用](https://github.com/xiph/vorbis/blob/c2aa86b05e981c96bf381fc6aa11cdd03eccc2fb/lib/codebook.c)，复用已链接的 libogg，消除自定义 writer 对任意字节偏移的整数写入及 MSVC reader 无条件八字节读取；保留 aoTuV 算法与质量参数，普通矩阵和 sanitizer 重新验证

上游未修版本的实际 sanitizer 结果为 FAIL：初始化触发 `psy.c` 的负值左移；仅回移前两项官方修复后的首例又触发 `codebook.c` 未对齐写入，恢复 libogg 后的完整 reader 依次揭示 libogg 移位和 sharedbook 提示码字移位；原 stderr、完整矩阵和构建身份保留，修补后的软件、sanitizer 及四目标验证独立取证，不沿用旧 PASS 宣称新二进制通过

Cargo 主版本范围保持不变，根 `[patch.crates-io]` 使用本地 sys 副本，`Cargo.lock` 固定实际依赖；升级绑定或替换副本时核对上游是否已包含修复，并重新运行对应检查，不能只因上游版本变动而丢弃修补

版权与 BSD 条款见原 Vorbis / libogg COPYING、源码 notice 及新增 LICENSE，LPC 独立 notice 随许可证目录收录；Symbian 配置的 CSIRO notice 保留在原文件，当前四目标不编译该配置
