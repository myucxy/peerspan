# Windows 原生模块

这里存放不能由 Tauri/Rust 控制层直接承载的 Windows 原生模块。每个模块应拥有独立的构建文件、接口说明和实机验证记录。

模块：

- `idd/`：可构建的 IddCx 1.2 间接显示驱动和诊断控制器，INF 最低目标为 Windows 10 1903；桌面端已接入软件设备生命周期，交换链通过 keyed-mutex 命名 D3D11 纹理发布最新 BGRA 帧。
- `crates/peerspan-video`：桌面进程中的共享纹理消费、GPU BGRA→NV12、Media Foundation 硬件编解码与原生 D3D11 接收窗口。
- `apps/desktop/src-tauri/src/input.rs`：认证会话下的 `SendInput` 注入、紧急释放和遗留窗口回迁。

未安装驱动时，桌面端只把虚拟显示器能力报告为“需安装”；已经实机验证的硬件媒体和输入适配器可独立报告就绪。任何情况下都不能用模拟画面或伪造 `Streaming` 状态代替真实首帧呈现。
