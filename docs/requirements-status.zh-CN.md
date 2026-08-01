# PeerSpan 需求与实现状态

更新日期：2026-08-01

本文把已确认的产品需求映射到当前代码和验证结果。只有接入真实系统能力并通过相应测试的项目才标记为“已完成”；仅有 UI 或接口边界的项目不视为完成。

## 首个 MVP

| 需求 | 状态 | 当前实现与验收依据 |
| --- | --- | --- |
| Windows 桌面客户端与耐用型 UI | 已完成 | React/Vite/Tauri 框架、设备页、屏幕布局、节点预览、设置诊断和配对对话框；已做多尺寸视觉检查 |
| 本机身份与配置持久化 | 已完成 | Ed25519 设备身份、SHA-256 指纹、偏好和可信设备存储；Rust 单元测试覆盖重启恢复 |
| 局域网自动发现 | 已完成 | `_peerspan._udp.local.` mDNS 与 2 秒轮询；双本地守护进程实测互相发现 |
| 安全配对引导 | 已完成 | 六位代码、SPAKE2、XChaCha20-Poly1305、Ed25519 签名身份交换；双端持久化集成测试通过 |
| DPI/缩放坐标映射 | 已完成 | 归一化内容坐标、边界钳制和不同 DPI 往返测试 |
| IddCx 1080p60 虚拟显示器 | 进行中 | 官方 IddSample 与 `native/idd` PeerSpan 原型均已无错误构建、INF 验证、CAT 生成和测试签名；单屏 1080p60 模式已固定，待实机安装、生命周期控制和 Windows 10 回移 |
| D3D11 帧获取和硬件编码 | 进行中 | IddCx 交换链线程已正确获取并释放 D3D11 帧；当前主动丢弃帧，硬件编码和传输尚未接入 |
| 认证媒体与长期控制通道 | 未开始 | 首次配对已安全实现；长期 TLS 1.3/QUIC 或等价通道待实现 |
| 远端解码显示与输入回传 | 未开始 | 协议和坐标规则已建立；输入注入、紧急释放与实机验证待实现 |
| 文本剪贴板 | 未开始 | 待在认证控制通道上实现，首版不传文件和富文本 |
| 断连清理与窗口恢复 | 未开始 | UI 会报告原生链路不可用；驱动显示器撤销和窗口回迁待实现 |
| Release 桌面构建 | 已完成 | `npm run build` 生成 `target\release\peerspan-desktop.exe` |

## 后续里程碑

- 窗口模式：远端代理窗口、生命周期同步、跨边缘交互和焦点仲裁。
- 应用节点模式：Node Service、用户会话 Agent、应用目录、远程启动和 ConPTY。
- 生产安全：Windows DPAPI 私钥保护、驱动签名与升级、权限隔离和安全审计。
- 质量目标：Windows 10/11 兼容矩阵、1080p60 稳定性、端到端延迟低于 80 ms、异常网络恢复和长时间运行测试。

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
