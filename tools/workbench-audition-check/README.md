# 工作台试听游标显示窄验

复用 Replay helper 的源码／extern 校验和 owned process group 清理，复制八个生产模块，仅注入窗口尺寸及 QA 系统；1280×800 和 640×480 各保存初始工具栏和独立试听游标两张 PNG，包和未保存草稿保持原值

```sh
python3 tools/workbench-audition-check/check.py prepare NEW_EVIDENCE_DIR
python3 tools/workbench-audition-check/check.py build NEW_EVIDENCE_DIR LAB_BUILD_JSON SOURCE_MANIFEST
python3 tools/workbench-audition-check/check.py run NEW_EVIDENCE_DIR VALIDATED_PACKAGE
```

试听游标是明确注入的显示控制，工具不打开输出设备；Kira 实际源位置、暂停／恢复和 canonical 起点另由 runtime 的 `canonical_start_positions_reject_edges_and_render_from_requested_source_frame` MockBackend 检查，实际输入主次、held、失焦和断开通过工作台 capture 窄测核对

PNG 仍须目检，本检查不代表工作台真实扬声器试听、物理手柄、输入延迟或真人验收；源码副本和确切旧 extern 的身份由各运行记录限定
