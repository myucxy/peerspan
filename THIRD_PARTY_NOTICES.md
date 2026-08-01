# PeerSpan 第三方组件与许可证

更新日期：2026-08-01

PeerSpan 用户态应用与原创 Rust/TypeScript 代码采用 `GPL-3.0-only`，完整条款见根目录 `LICENSE`。发布二进制时必须同步提供与该版本对应的完整源代码、构建脚本和许可证材料。

## Sunshine

- 上游：<https://github.com/LizardByte/Sunshine>
- 固定提交：`14ffa6fdaa53f7b51512be2b3d24f3939695403c`（`v2026.516.143833`）
- 安装包便携核心：`v2026.516.143833`，官方 ZIP SHA-256 `0A3AF3DDE43B8F2C94FFE04B850AD736D6E1BE2B75906779D7094A5AD9D4783B`
- 本地路径：`third_party/sunshine`
- 许可证：GNU General Public License v3.0，见子模块内 `LICENSE`
- 用途：Windows 显示捕获、硬件编码、GameStream/RTP/FEC、帧发送节奏、音频及远端输入主机核心

## Moonlight Qt

- 上游：<https://github.com/moonlight-stream/moonlight-qt>
- 固定提交：`f786e94c7b2f943e24e65d7d74deb539b827fc84`（`v6.1.0`）
- 安装包便携核心：`v6.1.0`，官方 ZIP SHA-256 `95F4D0853A31C7FCED4B6D233DDF55EE41720963F2E2620A9CB49A21D112AED1`
- 本地路径：`third_party/moonlight-qt`
- 许可证：GNU General Public License v3.0，见子模块内 `LICENSE`
- 用途：GameStream 客户端、硬件解码、低延迟呈现、输入采集和性能诊断

Moonlight Qt 还通过其自身子模块引用 `moonlight-common-c`、`qmdnsengine` 和 `SDL_GameControllerDB`。构建或发布 Moonlight 产物时必须递归取得这些依赖，并保留各依赖自己的许可证文件。

## VirtualDrivers Virtual Display Driver

- 上游：<https://github.com/VirtualDrivers/Virtual-Display-Driver>
- 审计源码固定提交：`d437ebc9b44a14ce6e5cc9c8b7f6beb08d6faf77`（release `25.7.23`）
- 本地路径：`third_party/virtual-display-driver`
- 安装包二进制：官方 `VirtualDisplayDriver-x86.Driver.Only.zip`；文件名沿用上游命名，包内 INF 实际目标为 `NTamd64`
- 官方 ZIP SHA-256：`E24210692B442B39AF763536330CE78B423F19342B7A7792C26DE3944E418B3A`
- 驱动版本：`11.30.4.434`；CAT 签名者：SignPath Foundation
- 许可证：MIT，见子模块内 `LICENSE`
- 用途：创建 Windows 10/11 IddCx 虚拟扩展屏，供 Sunshine 捕获并支持常用分辨率、刷新率和 HDR 能力

PeerSpan 不修改官方 INF、CAT 或 DLL，而是使用 INF 公开的 `MttVDD` 软件设备硬件 ID 管理显示器生命周期，因此不会破坏上游签名。安装脚本只在系统原先没有配置时写入一屏默认配置，并记录驱动、证书与配置所有权，卸载时保留安装前已经存在的 VDD。

## 旧 PeerSpan IddCx 原型（不进入当前安装包）

- 上游：微软 `windows-driver-samples/video/IndirectDisplay`
- 固定提交：`ef7c3074748ab05726c3a9161d3256118efd76e2`
- 本地路径：`native/idd`
- 许可证：Microsoft Public License，见 `native/idd/LICENSE.MS-PL`
- 状态：仅保留历史实现与原生媒体实验；当前 VDD + Sunshine/Moonlight 发布路径不构建或分发该测试签名驱动
