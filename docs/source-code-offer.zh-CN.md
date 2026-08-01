# PeerSpan 对应源代码获取说明

更新日期：2026-08-01

PeerSpan 用户态应用以 GNU GPL v3.0 only 发布。与安装包完全对应的 PeerSpan 源代码、构建脚本及固定的 Sunshine/Moonlight 子模块版本可从以下仓库获取：

```text
https://github.com/myucxy/peerspan
```

取得源码后执行：

```powershell
git submodule update --init --recursive
```

本版本使用：

- Sunshine `v2026.516.143833`，源码子模块提交 `14ffa6fdaa53f7b51512be2b3d24f3939695403c`；
- Moonlight Qt `v6.1.0`，源码子模块提交 `f786e94c7b2f943e24e65d7d74deb539b827fc84`；
- VirtualDrivers VDD release `25.7.23`，审计源码子模块提交 `d437ebc9b44a14ce6e5cc9c8b7f6beb08d6faf77`，按 MIT 许可证发布；
- 未进入当前安装包的旧 PeerSpan IddCx 微软派生原型独立按 Microsoft Public License 发布。

安装包中的 `licenses` 目录保留 GPLv3、Sunshine、Moonlight 与 VirtualDrivers VDD 的许可证。发行安装包时，必须同时发布该提交的源码归档或保持上述仓库及其递归子模块可访问；不得只分发二进制。
