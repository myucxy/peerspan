# PeerSpan IddCx 驱动原型

该目录包含一个可构建的 UMDF 2 间接显示驱动和软件设备控制器。当前原型创建一块稳定标识的虚拟显示器，首选模式为 `1920×1080@60Hz`，并保留 `1600×900@60Hz` 与 `1024×768@60Hz` 回退模式。

驱动的交换链线程目前只正确获取并释放 D3D11 帧，不编码或发送画面。它验证的是 IddCx 枚举、模式协商、帧边界和驱动打包，不能单独完成 PeerSpan 串流会话。

## 构建

```powershell
pwsh -File native\idd\build.ps1 -Configuration Release -Platform x64
```

脚本会寻找带 `Component.Microsoft.Windows.DriverKit` 的 Visual Studio 实例，并强制使用 64 位 MSBuild。WDK 26100 的 INF 验证器只有 x64/ARM64 版本，使用 32 位 MSBuild 会产生缺少 `x86\InfVerif.dll` 的误导性错误。

构建完成后，测试签名的 DLL、INF、CAT 和控制器位于 `native\idd\x64\Release`。这些文件是开发产物，不提交到 Git。安装测试签名驱动会改变系统驱动与证书状态，因此构建脚本不会自动安装。

当前工程使用 IddCx 1.4，INF 最低目标为 Windows 11 build 22000。PeerSpan 产品仍计划支持 Windows 10 1903；向 IddCx 1.0/1.2 回移并完成对应实机矩阵之前，不得声称 Windows 10 驱动兼容已完成。

## 来源与许可

驱动骨架改编自微软 `windows-driver-samples/video/IndirectDisplay`：

- 上游提交：`ef7c3074748ab05726c3a9161d3256118efd76e2`
- 上游许可：Microsoft Public License（见 `LICENSE.MS-PL`）
- PeerSpan 修改：设备标识、稳定容器 ID、单显示器 1080p60 模式、诊断信息、控制器错误处理和可复现构建脚本

本目录的上游派生代码继续遵循 MS-PL，不受 Rust 工作区的 MIT/Apache-2.0 声明覆盖。
