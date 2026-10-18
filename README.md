# pinapp

> Snipaste 风格的「贴图」(pin / sticky-image)工具,面向 **danklinux(Dank Material Shell / niri,Wayland)**,基于 [gpui-kit](https://gpui-kit.com) 用 Rust 开发。

`pinapp` 让你把截图「钉」在屏幕上:按下系统截图键截取画面后,再按一个热键,截图就会作为**始终置顶的悬浮窗**贴在桌面,方便对照、比对、做「半透明小抄」。

## 工作流

```
Print  ──►  dms screenshot(默认复制到剪贴板)  ──►  Pause  ──►  pinapp 读取剪贴板图片并钉到屏幕
```

- **Print**:danklinux 默认截图键,由 `dms screenshot` 接管,截图自动进剪贴板。
- **Pause**:由 niri 键位绑定到 `pinapp --pin`,程序从剪贴板取图并开一个置顶悬浮窗。

## 特性

- 贴图窗口:拖动移动、滚轮缩放、`Ctrl` + 滚轮调透明度
- 旋转 / 翻转(`R` / `Shift+R` / `F` / `V`)
- 鼠标穿透切换(`T`)——Wayland 下依赖合成器支持,见下方说明
- 多张贴图并存,`Esc` 关闭当前贴图
- 守护进程 + Unix Socket IPC,热键即时响应

## 安装

### 从 Release 下载

到 [Releases](https://github.com/Thy3634/pinapp/releases) 下载对应架构的 `pinapp-<target>.tar.gz`,解压后将 `pinapp` 放到 `PATH` 中。

### 从源码构建

```bash
# 依赖(Ubuntu/Debian 示例)
sudo apt install -y libclang-dev libgtk-3-dev libxkbcommon-dev \
  libwayland-dev libfontconfig1-dev libfreetype6-dev pkg-config libssl-dev

# wl-clipboard(dms 已自带,用于剪贴板读取)
sudo apt install -y wl-clipboard

cargo build --release
# 产物:target/release/pinapp
```

## 配置(niri)

把以下内容加入 `~/.config/niri/config.kdl`:

```kdl
# 让贴图窗口以浮动方式打开(天然置顶于平铺窗口之上)
window-rule {
    match app-id="pinapp"
    open-floating true
}

# 把 Pause 绑到 pinapp --pin
binds {
    Normal {
        bind "Pause" { spawn "pinapp"; spawn-arg "--pin"; }
    }
}
```

> niri 的键位语法请以你当前 niri 版本为准;若 `binds { Normal { ... } }` 不适用,可改用顶层 `bind "Pause" { spawn "pinapp"; spawn-arg "--pin"; }`。

## 开机自启

把 `pinapp`(无参数 = 守护进程)加入 niri / systemd --user 自启,例如 niri 的 `spawn-at-startup "pinapp"`,或写一段 systemd --user service。

## 用法

1. 按 **Print** 截图(dms 默认)。
2. 按 **Pause** 钉图。每按一次新增一张贴图。
3. 快捷键:

| 按键 | 作用 |
|---|---|
| 拖动窗口 | 移动 |
| 滚轮 | 缩放 |
| `Ctrl` + 滚轮 | 调透明度 |
| `R` / `Shift+R` | 顺时针 / 逆时针旋转 90° |
| `F` / `V` | 水平 / 垂直翻转 |
| `T` | 切换鼠标穿透 |
| `Esc` | 关闭当前贴图 |

## 已知限制

- **鼠标穿透**:Wayland / niri 普通应用没有标准的「输入穿透」API。开启 `T` 后程序会尽量不抢焦点,但真正点击穿透到下层窗口需要合成器层面支持;如遇不生效,可结合 niri 的窗口规则处理。
- 本程序面向 niri / Wayland;其它合成器需自行验证。

## 许可证

[MIT](LICENSE)
