# Linux 用户级安装

解压与机器架构一致的发行包后，可在解压目录运行 `./bin/cocobeat-game`；程序图标和桌面菜单入口通过以下命令安装到当前用户目录，无需管理员权限

```sh
install -Dm755 bin/cocobeat-game "$HOME/.local/bin/cocobeat-game"
cocobeat_data_dir="${XDG_DATA_HOME:-$HOME/.local/share}"
install -Dm644 share/applications/cocobeat.desktop "$cocobeat_data_dir/applications/cocobeat.desktop"
for size in 16 32 48 64 128 256 512 1024; do
    install -Dm644 "share/icons/hicolor/${size}x${size}/apps/cocobeat.png" "$cocobeat_data_dir/icons/hicolor/${size}x${size}/apps/cocobeat.png"
done
touch "$cocobeat_data_dir/icons/hicolor"
```

桌面入口通过 `PATH` 查找 `cocobeat-game`，安装前确保桌面会话的 `PATH` 包含 `$HOME/.local/bin`；修改登录环境后重新登录，应用菜单中会显示 CoCoBeat

桌面文件名、图标名与窗口 app_id 均为 `cocobeat`，PNG 使用标准 hicolor 尺寸目录；实际菜单、任务栏与 Wayland 图标显示需在对应桌面环境验收

发行包同时保留 `LICENSE`、`licenses/`、`assets/brand/README.md` 与 `assets/brand/PROVENANCE.csv`；README 引用的图片位于 `assets/brand/icons/symbol-256.png`
