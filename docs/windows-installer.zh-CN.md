# PeerSpan Windows 一体化安装包

更新日期：2026-08-02

## 内容与安全边界

一体化 NSIS 安装包可直接部署到 Windows 10/11 x64，目标机不需要 Node.js、Rust、Visual Studio 或 WDK。包内包含：

- PeerSpan 桌面应用及前端资源；
- VirtualDrivers VDD release `25.7.23` 中未经修改的正式签名 x64 IddCx 驱动 `11.30.4.434`；
- WebView2 Evergreen 离线安装程序；
- Sunshine `v2026.516.143833` 与 Moonlight Qt `v6.1.0` 官方 x64 便携核心；
- VDD 安装/卸载、配置所有权记录及仅限本地子网的防火墙脚本；
- GPLv3、MIT 和第三方许可证、固定源码提交及源码获取说明；
- 构建 commit、文件哈希、VDD 发布版本和签名状态清单。

“包含驱动”只指 VDD 虚拟显示驱动，不捆绑或覆盖 NVIDIA/AMD/Intel 显卡驱动和 Windows 系统组件。VDD 官方资产名为 `VirtualDisplayDriver-x86.Driver.Only.zip`，但包内 INF 明确声明 `NTamd64`；PeerSpan 只在 x64 构建中使用它。

PeerSpan 不修改 VDD 的 INF、CAT 或 DLL。安装时校验发布 ZIP SHA-256 `E24210692B442B39AF763536330CE78B423F19342B7A7792C26DE3944E418B3A`、每个包文件哈希以及 SignPath Foundation 的目录签名。无需启用 Windows 测试签名或全局忽略驱动签名。

## 构建

在项目根目录执行：

```powershell
$env:RUSTUP_HOME = "D:\Dev\Env\Rust\rustup"
$env:CARGO_HOME = "D:\Dev\Env\Rust\cargo"
$env:PATH = "D:\Dev\Env\Rust\cargo\bin;$env:PATH"

npm run build:installer
```

首次构建会下载固定版本的 VDD、Sunshine、Moonlight 和 WebView2。VDD/Sunshine/Moonlight 归档缓存于 `D:\Dev\Env\PeerSpan\downloads`，解压运行时位于 `D:\Dev\Env\PeerSpan\runtimes`，脚本每次使用前都校验固定 SHA-256。直连失败时可仅在当前终端使用代理：

```powershell
$env:HTTP_PROXY = "http://127.0.0.1:7897"
$env:HTTPS_PROXY = "http://127.0.0.1:7897"
npm run build:installer
```

输出位置：

```text
target\release\bundle\nsis\PeerSpan_0.1.0_x64-setup.exe
target\release\bundle\nsis\PeerSpan_0.1.0_x64-setup.exe.manifest.json
```

正式交付清单应满足 `gitDirty: false`、`testSignedDriver: false`，并记录 `vddRelease`、`vddDriverVersion` 与 `vddArchiveSha256`。复制到另一台机器前重新核对安装包 SHA-256。

## 安装与生命周期

1. 运行单个安装包并接受 UAC。
2. 安装脚本把正式签名 VDD 暂存到 Windows 驱动仓库；只在目录签名者尚未受信时把固定 SignPath 证书加入 `LocalMachine\TrustedPublisher`。
3. 只有系统不存在 `C:\VirtualDisplayDriver\vdd_settings.xml` 时才写入上游一屏默认配置，已有 VDD 用户配置不会被覆盖。
4. `%ProgramData%\PeerSpan\vdd-install-state.json` 记录驱动包、发布者证书和配置是否由 PeerSpan 创建。
5. PeerSpan 的“启用虚拟屏”使用上游 INF 公开的 `MttVDD` 硬件 ID 创建 `SWD\MttVDD\PeerSpanVirtualDisplay`。显示器只在租约存在时启用，撤销或应用退出时关闭。
6. 卸载只移除 PeerSpan 软件设备、PeerSpan 自己安装的 VDD 包/证书/配置和四条防火墙规则；安装前已经存在的 VDD 资源会保留。

升级时，脚本在签名 VDD 安全暂存后清理旧 `PeerSpanIdd.inf` 测试驱动、`SWD\PeerSpanVirtualDisplay` 节点，以及历史包中已记录的测试证书指纹 `9AADD10ED9B76B3934414EA36E4B1FEDCF701706`。当前安装包不再构建、信任或分发旧 PeerSpan 测试签名驱动。

## RDP 兼容说明

活动 RDP 会话会加载 `Microsoft Remote Display Adapter` / `RdpIdd_IndirectDisplay` 并占用当前显示拓扑。VDD 设备节点仍可能成功启动，但 Windows 可能延迟或拒绝把新屏加入该 RDP 桌面。PeerSpan 会保留已经启动的租约并报告“拓扑尚未出现”，不会把设备节点启动误判为验收通过。

优先使用物理控制台、Parsec/ Sunshine 等不替换 Windows 显示适配器的管理链路进行首次拓扑验收。若只能使用 RDP，应先安装，再断开（不要注销）RDP，随后从控制台或另一条远控链路启用虚拟屏；不应仅为测试擅自重启机器。

## 历史专用机记录

### 当前开发机 VDD 验证

2026-08-02 在 Windows 10 专业版 build 19045、NVIDIA GeForce RTX 5070 上完成：

- 旧 `PeerSpanIdd.inf`、`SWD\PeerSpanVirtualDisplay` 和本机测试证书已清理；
- 正式签名 VDD `11.30.4.434` 进入驱动仓库，Provider 为 `MikeTheTech`、Signer 为 `SignPath Foundation`；
- 与 Todesk 现有虚拟显示适配器并存时，PeerSpan 仍准确找到 VDD；
- 与 UI 共用同一后端的实机测试创建 `SWD\MttVDD\PeerSpanVirtualDisplay`，确认显示器进入扩展桌面并设置为 `1920×1080@60`，随后释放；
- 完整卸载后 VDD 包、PeerSpan 创建的配置、TrustedPublisher 证书和状态记录全部移除；重装后再次通过 `1920×1080@60` 测试；
- 一体化 EXE 静默覆盖安装退出码为 0，四条防火墙规则均为 Domain/Private、Inbound、Allow、RemoteAddress `LocalSubnet`，安装后的 `D:\Program Files\PeerSpan\peerspan-desktop.exe` 启动且窗口响应正常。

上述结果完成本机 VDD 和安装器链路，不代替两机真实 Sunshine/Moonlight 画面、输入和长稳验收。

### 192.168.9.26 旧包记录

2026-08-01，`192.168.9.26` 曾通过旧一体化包验证应用、`PeerSpanIdd.inf` 测试驱动、测试证书和 LocalSubnet 防火墙安装链路。SetupAPI 报告设备节点启动，但活动 RDP 以 `PNP_VetoOutstandingOpen` 阻止显示拓扑刷新，因此该记录不代表虚拟扩展屏或串流通过。

VDD 优化后的安装包必须重新在 `192.168.9.26` 验证，旧包 SHA-256 和 `oem9.inf` 只保留为迁移历史，不能作为当前交付证据。
