# 快速开始

三步走:

1. 按 <kbd>Print</kbd> 截图(dms 默认,截图自动进剪贴板)。
2. 按 <kbd>Pause</kbd> 钉图。每按一次新增一张贴图。
3. 拖动、缩放、调透明度,或按 <kbd>Esc</kbd> 关闭当前贴图。

> 提示:需在 niri 中把 <kbd>Pause</kbd> 绑到 `pinapp --pin`,并让 pinapp 随会话自启(守护进程)。详见下方配置。

## 交互与快捷键

| 操作            | 输入                          |
| --------------- | ----------------------------- |
| 移动            | 左键拖动                      |
| 放大 / 缩小     | 滚轮上 / 下                   |
| 透明度增 / 减   | **Ctrl** + 滚轮上 / 下        |
| 顺时针旋转      | `R`                           |
| 逆时针旋转      | `Shift` + `R`                 |
| 水平翻转        | `F`                           |
| 垂直翻转        | `V`                           |
| 切换鼠标穿透    | `T`                           |
| 关闭当前贴图    | `Esc`                         |

可以开任意多张贴图,每张都是独立窗口。`pinapp --quit` 关闭守护进程(关闭所有贴图)。

## niri 配置

将以下内容加入 `~/.config/niri/config.kdl`:

```kdl
// 让贴图窗口以浮动方式打开(天然置顶于平铺窗口之上)
window-rule {
    match app-id="pinapp"
    open-floating true
}

// 把 Pause 绑到 pinapp --pin(每个 spawn 参数独立加引号)
binds {
    Pause { spawn "pinapp" "--pin"; }
}
```

想在截图后自动贴图,可用:

```kdl
binds {
    Pause { spawn "sh" "-c" "dms screenshot && pinapp --pin"; }
}
```

niri ≥ 25.08 亦可写:

```kdl
binds {
    Pause { spawn-sh "dms screenshot && pinapp --pin"; }
}
```

> niri 的键位语法请以你当前 niri 版本为准。若 `spawn-arg` 之类写法报错,请改用上面的
> 「每个参数独立加引号」写法:`spawn "pinapp" "--pin";`。

## 开机自启(可选)

让守护进程随会话启动,这样第一次 `pinapp --pin` 立刻生效。任选其一:

**niri autostart**(若你的 niri 支持):

```kdl
autostart {
    "pinapp" "pinapp" "--serve"
}
```

**systemd --user**,drop-in `~/.config/systemd/user/pinapp.service`:

```ini
[Unit]
Description=pinapp clipboard-pin daemon

[Service]
ExecStart=%h/.cargo/bin/pinapp --serve
Restart=on-failure

[Install]
WantedBy=default.target
```

然后 `systemctl --user enable --now pinapp`。

客户端内置的自动拉起兜底意味着:即使不自启也只是首次稍慢,并非硬性要求。
