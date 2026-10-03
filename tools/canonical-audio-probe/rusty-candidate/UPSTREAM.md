# rusty_vorbis 研究候选来源

本目录固定使用 `rusty_vorbis 0.1.1` 官方发布包的 vendor 副本，并只修改 `src/frame.rs::forward_couple`；这是独立研究候选，不是上游原版或生产准入结论

## 官方发布包

- 名称与版本：`rusty_vorbis 0.1.1`
- 官方 archive：[rusty_vorbis-0.1.1.crate](https://static.crates.io/crates/rusty_vorbis/rusty_vorbis-0.1.1.crate)
- archive SHA256：`6eddf79e697ce9279ef0e012432cd2889a7b6305a45efb5bb16e8a1d6338646f`
- 上游仓库：[Remade-With-Rust/remade_ffmpeg_rs](https://github.com/Remade-With-Rust/remade_ffmpeg_rs)，包内路径 `crates/rusty_vorbis`
- 包内 VCS commit：`54df3fe0abfc6bda8589010d27102ecf81b1d106`，`.cargo_vcs_info.json` 同时标记 `dirty=true`，因此内容身份以官方 archive 哈希为准

已分别计算本机 Cargo cache archive 与当前官方 archive 下载内容的 SHA256，两者均匹配上述固定值；vendor 从 archive 的14个文件直接提取，保留 `Cargo.toml`、`Cargo.toml.orig`、`Cargo.lock`、`.cargo_vcs_info.json`、README、LICENSE、源码与 setup 二进制，不包含 registry 解包后生成的 `.cargo-ok` 或 `.cargo-checksum.json`

## 唯一修改

[max-abs-coupling.patch](patches/max-abs-coupling.patch) 只将 [forward_couple](vendor/rusty_vorbis/src/frame.rs) 的 magnitude 选择改为绝对值较大的声道残差，并根据 magnitude 符号计算 angle；未修改 `src/lib.rs` 或其他上游文件

- 补丁 SHA256：`28c3f3ed5d5f35ce239d0ea00c5aedf567859332cc521f665f46a0a5476848db`
- 官方 `src/frame.rs` SHA256：`5e53c8efbfedda59fc10abbe9b309f1c32c597d1b912eb4ed7ea42b7288b74aa`
- 候选 `src/frame.rs` SHA256：`c7959dbe8c4531305c564c528258ba6b62c35898967cd2cff4d2f6512d1dec0c`

补丁逐字节等于已冻结的研究补丁，重算官方文件与候选文件的 unified diff 也逐字节等于该补丁；其余13个文件逐字节保留，逐文件官方哈希、候选哈希与大小记录在 [upstream.json](upstream.json)

## PCM 输入约定

上游 [src/lib.rs](vendor/rusty_vorbis/src/lib.rs) 第256行将 `VorbisEncoder::push_pcm_f32` 的交错 F32 PCM 输入范围声明为 `[-1,1]`；第269–277行的实现初始化流后直接将样本加入声道缓冲，没有逐样本范围检查、有限性检查或 clamp

缺少上述检查是上游实现事实，不扩大其文档约定；本补丁没有改变该函数或输入幅度约定，调用入口应明确自己的输入检查与研究范围

## 许可与复核

上游包声明 `Apache-2.0`，完整 [LICENSE](vendor/rusty_vorbis/LICENSE) 逐字节保留，SHA256 为 `615853bfaaac274883e826894fa8ac2c441e6ff7d23733822b2992b8ea5bfd1a`；官方 archive 没有独立 NOTICE 文件，原 README、包元数据与源码中的上游说明均保留

本次只完成来源、文件集、许可保留、唯一函数差异与补丁一致性核对，没有重新执行音频实验；这些核对不替代候选入口的行为验证或音质验收
