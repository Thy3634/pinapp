# 安装

## 从 Release 下载

到 [Releases](https://github.com/Thy3634/pinapp/releases) 下载对应架构的
`pinapp-<target>.tar.gz`,解压后将 `pinapp` 放到 `PATH` 中。

## 从源码构建

依赖(Ubuntu / Debian 示例):

```sh
sudo apt install -y libclang-dev libgtk-3-dev libxkbcommon-dev \
  libwayland-dev libfontconfig1-dev libfreetype6-dev pkg-config libssl-dev
# wl-clipboard(dms 已自带,用于剪贴板读取)
sudo apt install -y wl-clipboard

cargo build --release
# 产物:target/release/pinapp
```

## 运行环境要求

- 一个 Wayland 合成器(概念上以 **niri** 测试)。
- **Rust** 工具链(edition 2021)用于构建。
- 剪贴板访问:
  - `arboard`(捆绑依赖),用于 X11 / 部分 Wayland 环境;**和/或**
  - [`wl-clipboard`](https://github.com/bugaevc/wl-clipboard) 提供的 `wl-paste` ——
    在纯 Wayland 会话、arboard 无法读取剪贴板时是必须的。`dms screenshot`(danklinux)正是写入这个剪贴板。

> 编译须在你的机器上完成 —— 编写本项目所用的沙箱无法编译 Rust。这里用到的每个 gpui-kit API
> 都已对着真实版本核对:`gpui-kit` **0.7.3** → 引擎 `gpui-pre` **0.3.9**(默认开启 Wayland + X11
> 后端)。仍请在你本机跑 `cargo build --release`,它才是真正的编译闸门。
