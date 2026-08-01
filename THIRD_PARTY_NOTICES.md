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

## PeerSpan IddCx

- 上游：微软 `windows-driver-samples/video/IndirectDisplay`
- 固定提交：`ef7c3074748ab05726c3a9161d3256118efd76e2`
- 本地路径：`native/idd`
- 许可证：Microsoft Public License，见 `native/idd/LICENSE.MS-PL`
- 用途：PeerSpan 虚拟扩展显示器和 D3D11 共享帧接口

IddCx 驱动是与用户态应用分离的独立二进制，不把 MS-PL 源码链接进 GPLv3 应用进程。
