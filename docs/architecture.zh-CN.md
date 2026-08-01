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
│  ├─ peerspan-media/       # 认证媒体数据报、分片、重放保护和低延迟重组
│  ├─ peerspan-protocol/    # 版本化的跨机控制消息
│  └─ peerspan-video/       # D3D11 与 Media Foundation 硬件视频管线
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

已实现桌面 UI、Web 设计预览、Tauri 命令桥、本机身份与可信设备持久化、偏好持久化、协议基础类型、配对码保护、TLS 控制通道和 DPI 归一化坐标规则。

局域网发现使用 `_peerspan._tcp.local.` mDNS 服务，桌面端每 2 秒刷新一次协议 v3 兼容节点。mDNS 的主服务端口为 TLS 控制端口 `37622`，TXT 记录另外发布配对端口 `37621`。配对引导使用 120 秒有效的六位代码和 SPAKE2 协商共享秘密，以 XChaCha20-Poly1305 保护身份交换，并用 Ed25519 签名验证持久设备身份；单次配对邀请最多允许 5 次尝试。发现测试和本机双端配对测试均已通过。

长期控制通道仅启用 TLS 1.3，ALPN 为 `peerspan-control/3`。客户端和服务端都使用本机长期 Ed25519 身份签发的自签名证书，并按配对时保存的 Ed25519 公钥固定证书 SPKI；服务端在每次握手时读取最新可信设备列表，因此未配对或已撤销的客户端会在 TLS 握手阶段被拒绝。TLS 建立后，协议 Hello 还会再次核对协议版本、设备 UUID、指纹和证书公钥，防止发现记录或应用层身份与证书错配。

当前控制会话支持 Display Offer/Decision、媒体端口协商、500 ms 心跳、往返延迟统计、主动 Session End 和双端清理。核心层只允许一个活动显示会话并限制状态合法迁移。媒体硬件编解码和输入链路尚未接通时会明确拒绝相应能力；已认证的显示会话保持 `Negotiating`，只有真实媒体被接收并呈现后才能进入 `Streaming`。

`peerspan-media` 已建立编码视频访问单元的数据报边界：单个 UDP 数据报不超过 1200 字节，以 ChaCha20-Poly1305 认证加密并绑定会话 UUID、包序号、帧号、分片序号和时间戳；接收端提供 128 包重放窗口、乱序重组、8 MiB 单帧上限、最多 4 个在途帧和 80 ms 过期丢弃。Windows UDP 套接字显式申请 4 MiB 收发缓冲，避免大关键帧突发超过系统默认缓冲。双方从已认证 TLS 1.3 连接以会话 UUID 作为上下文导出 36 字节密钥材料，接收端绑定临时 UDP 端口并通过 Display Decision 返回，控制会话结束时双端同步释放套接字和清零密钥。双本地实例已经传输并还原 70 KB 加密访问单元；真实硬件编码器与解码呈现仍待接入，因此生产能力当前不会标记为就绪。

`peerspan-video` 负责 Windows 原生视频能力边界。桌面启动时会创建带视频支持的真实 D3D11 硬件设备和 Media Foundation DXGI 设备管理器，并分别枚举 D3D11-aware 的 H.264 编码/解码 MFT；编码端只接受硬件 MFT，解码端允许使用 Windows 自带但由 D3D11/DXVA 加速的解码 MFT。探测结果会写入诊断能力，但在 IDD 纹理交接、颜色转换和实时编解码实际接通前仍保持“开发中”，不会仅凭安装了编解码器就允许串流。可用 `cargo run -p peerspan-video --example probe` 独立复核本机结果。

本机 Ed25519 身份私钥使用当前 Windows 用户作用域的 DPAPI 加密后写入 `identity.json`，并附加 PeerSpan 固定应用熵。文件通过同目录临时文件和原子替换更新，避免写入中断留下半份身份。旧版 `signingKeyHex` 明文格式会在首次成功读取时立即迁移为 `protectedSigningKeyHex`，设备 ID 与密钥保持不变；DPAPI 数据损坏或换到其他 Windows 用户后无法解密时，应用明确报错且不会静默生成新身份，以免破坏既有信任关系。

`native/idd` 已包含可构建的 IddCx 1.4 UMDF 驱动原型和独立诊断控制器，固定枚举一块 1080p60 虚拟显示器。桌面后端直接调用 Windows `SwDeviceCreate`，只有异步创建成功且 Configuration Manager 确认设备节点进入 `DN_STARTED` 后才报告能力就绪；用户撤销或应用退出时关闭句柄，活动会话期间拒绝撤销。开发安装/卸载脚本需要管理员身份、`ShouldProcess` 确认和额外的系统变更确认开关，普通构建不会安装驱动或信任测试证书。

当前开发机尚未安装 PeerSpan 驱动，因此已完成的是生命周期 API、错误诊断、单元测试和 Release 驱动构建，实机显示器枚举仍待专用测试机验收。交换链线程已具备正确的 D3D11 帧获取/释放边界，但仍会丢弃帧；纹理交给硬件编码器、远端输入、剪贴板和断连恢复仍属于待实现的原生链路。该驱动当前最低目标是 Windows 11 build 22000，Windows 10 1903 支持需要回移 IddCx 版本并单独验证。
