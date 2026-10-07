# 工作台试听游标显示窄验

复用 Replay helper 的源码／extern 校验和 owned process group 清理，复制八个生产模块，仅注入窗口尺寸及 QA 系统；1280×800 和 640×480 各保存初始工具栏和独立试听游标两张 PNG，包和未保存草稿保持原值

```sh
python3 tools/workbench-audition-check/check.py prepare NEW_EVIDENCE_DIR
python3 tools/workbench-audition-check/check.py build NEW_EVIDENCE_DIR LAB_BUILD_JSON SOURCE_MANIFEST
python3 tools/workbench-audition-check/check.py run NEW_EVIDENCE_DIR VALIDATED_PACKAGE
```

试听游标是明确注入的显示控制，工具不打开输出设备；Kira 实际源位置、暂停／恢复和 canonical 起点另由 runtime 的 `canonical_start_positions_reject_edges_and_render_from_requested_source_frame` MockBackend 检查，实际输入主次、held、失焦和断开通过工作台 capture 窄测核对

PNG 仍须目检，本检查不代表工作台真实扬声器试听、物理手柄、输入延迟或真人验收；源码副本和确切旧 extern 的身份由各运行记录限定

新增 `run-audio` 模式保留上述显示控制历史证据，另通过合成 `KeyboardInput`／`WindowFocused` 走未修改的生产 capture、audition Output 和 Kira CPAL；生产读取的 callback 源位置与状态用于断言，工具不写试听位置、播放状态或选择

```sh
python3 tools/workbench-audition-check/check.py prepare NEW_EVIDENCE_DIR
python3 tools/workbench-audition-check/check.py build-frozen NEW_EVIDENCE_DIR FROZEN_ARTIFACTS_DIR/freeze.json
python3 tools/workbench-audition-check/check.py run-audio NEW_EVIDENCE_DIR VALIDATED_PACKAGE
```

`freeze.json` 记录成功 lab JSON 构建中的八个原模块、精确 rlib／proc-macro 和本次固定生产二进制身份；依赖与本地静态 archive 先复制到独立目录再校验，helper 用 rustc 直接链接副本，生产构建输入中的后续标签编辑须在观察记录中单列；系统动态库及音频服务器仍属于当前宿主环境

只读 Replay 使用原始负帧 watermark 验证错误保留，随后实际输入选择零帧启动、等待 callback 前进、暂停确认及 300 ms 游标稳定、暂停中 seek 保持旧观测位置、从新目标恢复、失焦暂停和 Stop；两个尺寸保存实际状态 PNG，第三个子进程仅用空 `ALSA_CONFIG_PATH` 验证真实输出初始化失败，配置仅影响 owned 子进程

成功记录限定为本机真实软件音频 callback 接线，不代表扬声器声学输出、物理键盘／手柄、计时校准或真人试听；PNG 仍需目检，失败目录和日志保留，所有输出使用新目录
