# QUIC 会话检查

从仓库根目录运行，使用已构建的 lab 与新的输出目录，不重新编码 fixture、不依赖第三方 Python 包，输出目录需保留以检查失败日志

```sh
cargo build --locked -p cocobeat-lab
python3 tools/quic-session-check/check.py target/debug/cocobeat-lab target/quic-session-check
```

脚本仅绑定本机 loopback，检查原字节资源接收、预装同包加入、可靠历史与权威 Replay，以及模板错配和目标路径保护；受限制的 executor 需获得 loopback socket 访问授权

实际源音频来自有来源记录的 `testdata/synthetic/media-import/stereo-canonical.ogg`，0.1 秒、4800 帧；合成输入用于验证协议和 core 一致性，不代表音乐质量、设备精度、真实双机或真人体验
