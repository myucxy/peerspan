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
| MSVC | 19.44.35228 | `D:\Dev\Env\VisualStudio\BuildTools\VC\Tools\MSVC` | Windows x64 原生编译 | 在 VS 开发者终端执行 `cl` |
| Windows SDK | 10.0.26100.0 | `C:\Program Files (x86)\Windows Kits\10` | Windows 头文件、库与工具 | `Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0'` |
| Windows Driver Kit | 10.1.26100.6584 | `C:\Program Files (x86)\Windows Kits\10` | IddCx 间接显示驱动开发 | 检查 `Include\10.0.26100.0\um\IddCx.h` 与 `build\10.0.26100.0` |

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

`IddCx.h` 和 WDK MSBuild targets 已安装，但官方 `IddSampleDriver.sln` 在当前自定义 Build Tools 实例上仍报告缺少 `WindowsApplicationForDrivers10.0` 和 `WindowsUserModeDriver10.0` 平台工具集。该问题属于 WDK 与自定义 Visual Studio Build Tools 位置的 MSBuild 集成，修复前不要通过复制工具集文件绕过，也不要将驱动示例已编译误记为完成。

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
```
