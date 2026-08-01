# PeerSpan 工程架构

## 目录结构

```text
peerspan/
├─ apps/
│  └─ desktop/              # React UI 与 Tauri Windows 桌面壳
│     ├─ src/               # 视图、组件、状态和 Tauri 调用适配
│     └─ src-tauri/         # 桌面进程入口与平台适配组合
├─ crates/
│  ├─ peerspan-core/        # 与 UI/传输实现无关的领域规则和配置
│  └─ peerspan-protocol/    # 版本化的跨机控制消息
├─ native/                  # IddCx、D3D11 和输入等 Windows 原生模块
└─ docs/                    # 需求、架构、环境和验证记录
```

## 分层约束

1. React 只处理显示和用户意图，不直接实现网络、驱动或输入注入。
2. Tauri command 是 UI 与本机核心层之间的窄接口，返回可序列化的真实状态。
3. `peerspan-core` 不依赖 Tauri，核心状态转换、DPI 坐标规则和配置持久化可独立测试。
4. `peerspan-protocol` 中的消息必须显式版本化；媒体流与控制流分离。
5. `native` 适配器在没有通过实机验证时必须报告不可用，禁止静默回退成伪会话。

## 当前实现边界

已实现桌面 UI、Web 设计预览、Tauri 命令桥、本机身份与可信设备持久化、偏好持久化、协议基础类型、配对码保护和 DPI 归一化坐标规则。

局域网发现使用 `_peerspan._udp.local.` mDNS 服务，桌面端每 2 秒刷新一次兼容节点。配对引导使用 120 秒有效的六位代码和 SPAKE2 协商共享秘密，以 XChaCha20-Poly1305 保护身份交换，并用 Ed25519 签名验证持久设备身份；单次配对邀请最多允许 5 次尝试。发现测试和本机双端配对测试均已通过。

当前密码学配对只负责首次信任引导。长期控制和媒体通道仍需使用 TLS 1.3/QUIC 或等价的认证加密传输；本机身份私钥后续还需要接入 Windows DPAPI。

`native/idd` 已包含可构建的 IddCx 1.4 UMDF 驱动原型和软件设备控制器，固定枚举一块 1080p60 虚拟显示器。当前交换链线程已具备正确的 D3D11 帧获取/释放边界，但会丢弃帧；驱动安装/撤销、纹理交给硬件编码器、远端输入、剪贴板和断连恢复仍属于待实现的原生链路。该驱动当前最低目标是 Windows 11 build 22000，Windows 10 1903 支持需要回移 IddCx 版本并单独验证。
