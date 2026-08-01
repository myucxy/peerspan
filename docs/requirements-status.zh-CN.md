# PeerSpan 需求与实现状态

更新日期：2026-08-01

本文把已确认的产品需求映射到当前代码和验证结果。只有接入真实系统能力并通过相应测试的项目才标记为“已完成”；仅有 UI 或接口边界的项目不视为完成。

## 首个 MVP

| 需求 | 状态 | 当前实现与验收依据 |
| --- | --- | --- |
| Windows 桌面客户端与耐用型 UI | 已完成 | React/Vite/Tauri 框架、设备页、屏幕布局、节点预览、设置诊断和配对对话框；已做多尺寸视觉检查 |
| 本机身份与配置持久化 | 已完成 | Ed25519 设备身份、SHA-256 指纹、偏好和可信设备存储；私钥使用当前 Windows 用户作用域 DPAPI 保护，旧明文格式会无损迁移，损坏数据拒绝自动轮换；Rust 单元测试覆盖新建、重启、迁移和异常路径 |
| 局域网自动发现 | 已完成 | `_peerspan._tcp.local.` mDNS、协议 v4 与 2 秒轮询；发布控制端口 `37622` 和配对端口 `37621`，双本地守护进程实测互相发现 |
| 安全配对引导 | 已完成 | 六位代码、SPAKE2、XChaCha20-Poly1305、Ed25519 签名身份交换；双端持久化集成测试通过 |
| DPI/缩放坐标映射 | 已完成 | 归一化内容坐标、边界钳制和不同 DPI 往返测试 |
| IddCx 1080p60 虚拟显示器 | 进行中 | 单屏驱动、构建/INF/CAT/测试签名、显式开发安装、`SwDeviceCreate` 生命周期与 `DN_STARTED` 校验均已实现；工程已回移到 IddCx 1.2、INF 最低 build 18362，并通过当前 WDK Release 构建。开发机未安装驱动，仍待 Windows 10/11 专用机枚举验收 |
| D3D11 帧获取和硬件编码 | 进行中 | IddCx 零等待 keyed-mutex 发布、GPU 本地复制、GPU NV12 转换、NVIDIA H.264 硬编和 Windows 硬解闭环均已实机通过；真实 IddCx 帧的最后一跳仍受未安装驱动限制，待专用机验收 |
| 认证媒体与长期控制通道 | 已完成 | 协议 v4 双向 TLS 1.3、exporter 会话密钥、临时 UDP 端口、1200 字节 AEAD 分片、重放保护、有界乱序重组、真实共享纹理编码发送 worker、硬解接收 worker 和首帧呈现确认均已接入 |
| 远端解码显示与输入回传 | 已完成 | 原生 Win32/D3D11 双缓冲交换链窗口、同设备硬解纹理→GPU BGRA→`Present` 的无 CPU 读回生产路径、归一化鼠标、滚轮、扫描码键盘、`SendInput` 注入、可配置紧急释放和断连全键释放均已实现；硬编→硬解→`Present` 实机测试通过 |
| 文本剪贴板 | 已完成 | 双向认证 TLS 文本同步、1 MiB 上限、修订号去重和本机写入循环抑制；不传图片、富文本或文件 |
| 断连清理与窗口恢复 | 已完成 | 1 秒链路失活判断、媒体/窗口/输入资源清理、虚拟屏遗留窗口回迁主屏，以及前端每 2 秒随发现状态持续自动重连均已实现 |
| Release 桌面构建 | 已完成 | `npm run build` 生成 `target\release\peerspan-desktop.exe` |

## 后续里程碑

- 窗口模式：远端代理窗口、生命周期同步、跨边缘交互和焦点仲裁。
- 应用节点模式：Node Service、用户会话 Agent、应用目录、远程启动和 ConPTY。
- 生产安全：DPAPI 私钥保护已完成；驱动生产签名与升级、权限隔离和安全审计仍待完成。
- 质量目标：Windows 10/11 兼容矩阵、1080p60 稳定性、端到端延迟低于 80 ms、异常网络恢复和长时间运行测试。

## 外部验收边界

当前开发机未安装 PeerSpan 测试驱动，且仓库不会自动修改测试签名或系统信任。因此代码、硬件编解码、共享纹理、D3D11 呈现和驱动构建已经完成验证；“真实 IddCx 帧跨两台电脑”和 Windows 10/11 驱动安装矩阵必须在允许安装测试驱动的专用机器上按 [Windows 专用机验收手册](windows-acceptance.zh-CN.md) 完成，不能用模拟画面替代。

## 提交前验证命令

```powershell
$env:RUSTUP_HOME = "D:\Dev\Env\Rust\rustup"
$env:CARGO_HOME = "D:\Dev\Env\Rust\cargo"
$env:PATH = "D:\Dev\Env\Rust\cargo\bin;$env:PATH"

cargo fmt --all
cargo test --workspace
cargo test --workspace -- --ignored
npm run typecheck
npm test
npm run build:web
npm run build
pwsh -File native\idd\build.ps1 -Configuration Release -Platform x64
```
