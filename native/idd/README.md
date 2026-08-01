# PeerSpan IddCx 驱动原型

该目录包含一个可构建的 UMDF 2 间接显示驱动和软件设备控制器。当前原型创建一块稳定标识的虚拟显示器，首选模式为 `1920×1080@60Hz`，并保留 `1600×900@60Hz` 与 `1024×768@60Hz` 回退模式。

驱动交换链线程把最新 `DXGI_FORMAT_B8G8R8A8_UNORM` 帧复制到按分辨率命名的 D3D11 NT 共享纹理（例如 `Global\PeerSpan.Idd.Frame.v1.1920x1080`）。keyed mutex 的 key 0 属于驱动、key 1 属于桌面消费者；驱动使用零超时获取 key 0，上一帧未消费时直接丢弃新帧，绝不阻塞 DWM。共享句柄 ACL 只授权 SYSTEM、管理员和交互式用户，不向网络或匿名主体开放。

桌面端取得 key 1 后先做一次进程内 GPU 复制并立刻归还 key 0，再通过 Media Foundation Video Processor 在 GPU 上将 BGRA 转为 NV12 并交给硬件 H.264 编码器；整个发送路径没有 CPU 像素读回。共享纹理、key 交接、GPU 转换和硬件编码已用两个独立 D3D11 设备做自动化实机测试。专用机 `192.168.9.26` 已完成驱动包安装并记录设备启动成功，但活动 RDP 阻止显示拓扑刷新；重启后的真实 IddCx 交换链到桌面进程最后一跳仍待验收。

## 构建

```powershell
pwsh -File native\idd\build.ps1 -Configuration Release -Platform x64
```

脚本会寻找带 `Component.Microsoft.Windows.DriverKit` 的 Visual Studio 实例，并强制使用 64 位 MSBuild。WDK 26100 的 INF 验证器只有 x64/ARM64 版本，使用 32 位 MSBuild 会产生缺少 `x86\InfVerif.dll` 的误导性错误。

下行 Windows 10 不提供可供第三方 INF `Include/Needs` 的 `WUDFRD.inf`，因此 PeerSpan 与系统自带 `rdpidd.inf` 一样直接把现有 `WUDFRd.sys` 注册为函数驱动。WDK 26100 会对此报告 InfVerif 2084（系统服务二进制不在本包 `CopyFiles` 中）；构建脚本只把 2084 降为消息，其他警告和 Catalog signability 检查仍保留。不得为了消除该诊断而把 Windows 的 `WUDFRd.sys` 复制进 PeerSpan 驱动包。

构建完成后，测试签名的 DLL、INF、CAT 和控制器位于 `native\idd\x64\Release`。这些文件是开发产物，不提交到 Git。安装测试签名驱动会改变系统驱动与证书状态，因此构建脚本不会自动安装。

## 开发安装与生命周期

桌面后端直接使用 Windows `SwDeviceCreate` 创建软件设备，不依赖或启动外部控制器进程。只有回调成功且 Configuration Manager 报告设备节点进入 `DN_STARTED` 后，虚拟显示器能力才会标记为就绪；撤销、应用退出或创建失败时会关闭软件设备句柄。`PeerSpanIddController.exe` 仅保留为独立诊断工具。

安装和卸载脚本默认拒绝执行，必须在管理员 PowerShell 中显式确认系统变更。开发测试证书只应在专用测试机使用；普通开发构建和自动验证不得运行这些命令：

```powershell
# 先阅读脚本。以下命令会修改本机驱动仓库和证书信任。
pwsh -File native\idd\install-dev.ps1 -Configuration Release -Platform x64 `
  -TrustTestCertificate -AcknowledgeSystemChanges

# 先退出 PeerSpan，再移除开发驱动；证书移除仍需显式开关。
pwsh -File native\idd\uninstall-dev.ps1 -Configuration Release -Platform x64 `
  -RemoveTestCertificate -AcknowledgeSystemChanges
```

生产环境不得信任项目生成的测试证书，必须改用正式签名与安装器。当前开发机尚未安装该驱动，因此本轮只验证了 API 封装、失败路径、生命周期单元测试和驱动构建，未声称实机显示器已验收。

专用测试机可直接使用根目录的 `npm run build:installer` 生成一体化 NSIS 包，不需要单独复制本节的驱动文件或运行脚本。安装器会同时部署应用、测试签名驱动、匹配证书、WebView2 离线运行时和仅限本地子网的防火墙规则；详细边界与远端记录见 [Windows 安装包说明](../../docs/windows-installer.zh-CN.md)。

当前工程已回移到 IddCx 1.2，INF 最低目标为 Windows 10 1903 build 18362，并通过 WDK 10.0.26100 Release x64 构建、Catalog 和测试签名。`192.168.9.26` 已验证驱动包暂存、设备配置和设备启动；RDP 显示适配器造成 Windows 要求重启，因此 1080p60 桌面拓扑、Windows 10/11 兼容矩阵和长稳测试仍未形成发布声明。

## 来源与许可

驱动骨架改编自微软 `windows-driver-samples/video/IndirectDisplay`：

- 上游提交：`ef7c3074748ab05726c3a9161d3256118efd76e2`
- 上游许可：Microsoft Public License（见 `LICENSE.MS-PL`）
- PeerSpan 修改：设备标识、稳定容器 ID、单显示器 1080p60 模式、最新帧共享纹理、诊断信息、控制器错误处理和可复现构建脚本

本目录的上游派生代码继续遵循 MS-PL，不受 Rust 工作区的 MIT/Apache-2.0 声明覆盖。
