# PeerSpan 开发环境

更新日期：2026-08-01

## 环境约定

系统级开发环境统一安装在 `D:\Dev\Env`。新增或调整环境时，记录名称、版本、路径、用途、环境变量和验证命令。项目包管理器能够还原的依赖不放入该目录。

## 已验证工具

| 工具 | 版本 | 路径 | 用途 | 验证命令 |
| --- | --- | --- | --- | --- |
| Node.js | 24.14.1 | `D:\Dev\Env\nvm4w\nodejs` | React/Vite 前端与 Tauri CLI | `node --version` |
| npm | 11.12.1 | 随 Node.js 安装 | JavaScript 工作区依赖 | `npm --version` |
| Git | 2.41.0.windows.3 | `D:\Dev\Env\Git` | 版本控制 | `git --version` |
| Rust | 1.97.1 (`x86_64-pc-windows-msvc`) | `D:\Dev\Env\Rust` | 核心层与 Tauri 后端 | `D:\Dev\Env\Rust\cargo\bin\rustc.exe --version` |
| Cargo | 1.97.1 | `D:\Dev\Env\Rust\cargo` | Rust 工作区构建与测试 | `D:\Dev\Env\Rust\cargo\bin\cargo.exe --version` |
| Edge WebView2 Runtime | 150.0.4078.105 | 系统运行时 | Tauri WebView 渲染 | 查询注册表 `EdgeUpdate\Clients` |
| Visual Studio Build Tools 2022 | 17.14.37 | `D:\Dev\Env\VisualStudio\BuildTools` | MSVC 链接器与 Windows SDK | `VC\Auxiliary\Build\vcvars64.bat` 后执行 `cl` |
| Visual Studio Community 2022 | 17.14.37 | `D:\Dev\Env\VisualStudio\Community` | WDK VSIX、驱动工程与 64 位 MSBuild | `vswhere -requires Component.Microsoft.Windows.DriverKit` |
| MSVC | 19.44.35228 | `D:\Dev\Env\VisualStudio\BuildTools\VC\Tools\MSVC` | Windows x64 原生编译 | 在 VS 开发者终端执行 `cl` |
| MSVC Spectre 缓解库 | 14.44.35207 x64 | `D:\Dev\Env\VisualStudio\Community\VC\Tools\MSVC\14.44.35207\lib\spectre\x64` | WDK 驱动 Release/Debug 链接 | 检查目录后构建 `native\idd` |
| Windows SDK | 10.0.26100.0 | `C:\Program Files (x86)\Windows Kits\10` | Windows 头文件、库与工具 | `Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0'` |
| Windows Driver Kit | 10.1.26100.6584 | `C:\Program Files (x86)\Windows Kits\10` | IddCx 间接显示驱动开发 | 检查 `Include\10.0.26100.0\um\iddcx\1.4\IddCx.h` 与 `build\10.0.26100.0` |
| Visual Studio WDK 组件 | 10.0.26100.16 | `D:\Dev\Env\VisualStudio\Community` | 驱动项目模板、平台工具集与 VS 集成 | `vswhere -requires Component.Microsoft.Windows.DriverKit` |

Windows DPAPI 由操作系统提供，本机身份私钥保护不需要在 `D:\Dev\Env` 追加系统级工具。Rust 绑定由 `windows-sys` 项目依赖锁定并随 Cargo 还原。

TLS 控制通道使用 `rustls`、`rcgen` 和 `x509-parser`。它们都是由 `Cargo.toml` 与 `Cargo.lock` 锁定、通过 Cargo 还原的项目依赖，不需要在 `D:\Dev\Env` 安装额外的 TLS 库或系统工具；其中 `rustls` 使用项目选定的 `ring` 加密提供程序。

虚拟显示器桌面生命周期使用 Windows 自带的 Software Device 与 Configuration Manager API，Rust 绑定由现有 `windows-sys` 依赖提供，不需要向 `D:\Dev\Env` 添加工具。驱动开发安装脚本会修改系统驱动仓库，选择信任测试证书时还会修改本机证书库，因此不属于常规构建步骤，自动验证不得执行。

媒体数据报核心使用 Cargo 管理的 `chacha20poly1305`、`socket2` 和 `zeroize`，不需要新增系统环境；Windows 套接字缓冲调整仍由操作系统 API 完成。

Windows 视频管线使用 Cargo 管理的 `windows` crate 调用系统自带 D3D11 与 Media Foundation，不需要向 `D:\Dev\Env` 安装额外 SDK。独立探测命令为 `cargo run -p peerspan-video --example probe`；2026-08-01 在当前 Windows 10 22H2 开发机上确认 D3D 11.1、`NVIDIA H.264 Encoder MFT` 与 `Microsoft H264 Video Decoder MFT` 均为 D3D11-aware，并完成 640×360 NV12 → 硬件 H.264 关键帧 → D3D11 解码 NV12 的实机闭环。对应硬件测试可用 `cargo test -p peerspan-video -- --ignored` 重跑。

## Rust 环境变量

当前 Rust 使用项目外的固定目录：

```powershell
$env:RUSTUP_HOME = "D:\Dev\Env\Rust\rustup"
$env:CARGO_HOME = "D:\Dev\Env\Rust\cargo"
$env:PATH = "D:\Dev\Env\Rust\cargo\bin;$env:PATH"
```

当前自动化终端不会始终继承上述 `PATH`，运行 Cargo 前应显式执行这三行。需要编译依赖 MSVC 的目标时，可先加载 Build Tools 环境：

```powershell
cmd /c '"D:\Dev\Env\VisualStudio\BuildTools\VC\Auxiliary\Build\vcvars64.bat" && set' |
  ForEach-Object {
    if ($_ -match '^(.*?)=(.*)$') { Set-Item -Path "Env:$($matches[1])" -Value $matches[2] }
  }
```

## WDK 记录

WDK 安装器不支持完全重定向到 `D:\Dev\Env`，因此这是系统环境目录约定的一个安装器强制例外，实际文件位于 `C:\Program Files (x86)\Windows Kits\10`。官方驱动示例使用稀疏检出保存在：

```text
D:\Dev\Env\WindowsDriverKit\windows-driver-samples
commit ef7c3074748ab05726c3a9161d3256118efd76e2
sample video\IndirectDisplay
```

从 Visual Studio 17.11 起，WDK VSIX 是 Visual Studio Installer 中的独立组件，不会自动附加到仅有命令行 Build Tools 的实例。为此新增了完整 Community 实例，并安装以下组件：

```text
Microsoft.VisualStudio.Workload.NativeDesktop
Component.Microsoft.Windows.DriverKit
Microsoft.VisualStudio.Component.VC.14.44.17.14.x86.x64.Spectre
```

安装器保存在：

```text
D:\Dev\Env\VisualStudio\Installer\vs_community.exe
D:\Dev\Env\WindowsDriverKit\installer\wdksetup-10.1.26100.6584.exe
```

验证命令：

```powershell
$vswhere = "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"
& $vswhere -products "*" -requires Component.Microsoft.Windows.DriverKit -property installationPath
pwsh -File native\idd\build.ps1 -Configuration Release -Platform x64
```

微软官方 `video\IndirectDisplay\IddSampleDriver.sln` 已在 x64 Release 下完成无错误、无警告构建、INF 验证、CAT 生成和测试签名。WDK 26100 的 INF 验证 DLL 只有 x64/ARM64 版本，必须使用 `MSBuild\Current\Bin\amd64\MSBuild.exe`；32 位 MSBuild 会报告缺少 `x86\InfVerif.dll`。`native\idd\build.ps1` 已固定这一选择并禁用 MSBuild 节点复用。

若依赖下载直连失败，可只在当前终端临时设置代理，不应提交到项目配置：

```powershell
$env:HTTP_PROXY = "http://127.0.0.1:7897"
$env:HTTPS_PROXY = "http://127.0.0.1:7897"
```

## 常用命令

```powershell
npm install
npm run dev:web
npm run typecheck
npm test

$env:RUSTUP_HOME = "D:\Dev\Env\Rust\rustup"
$env:CARGO_HOME = "D:\Dev\Env\Rust\cargo"
$env:PATH = "D:\Dev\Env\Rust\cargo\bin;$env:PATH"
cargo test --workspace
cargo test --workspace -- --ignored
npm run dev
npm run build
pwsh -File native\idd\build.ps1 -Configuration Release -Platform x64
```
