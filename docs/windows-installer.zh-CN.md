# PeerSpan Windows 一体化测试安装包

更新日期：2026-08-01

## 目标与边界

一体化安装包用于把 PeerSpan 直接部署到专用 Windows 测试机，不要求目标机安装 Node.js、Rust、Visual Studio 或 WDK。最终离线包包含：

- PeerSpan 桌面应用及前端资源；
- PeerSpan IddCx 虚拟显示驱动的 `INF`、测试签名 `CAT` 和 UMDF `DLL`；
- 与该驱动包匹配的 PeerSpan 测试证书；
- WebView2 Evergreen 离线安装程序；
- Sunshine `v2026.516.143833` 与 Moonlight Qt `v6.1.0` 的官方 x64 便携核心；
- 驱动、证书和仅限本地子网的防火墙规则安装/卸载脚本；
- GPLv3/第三方许可证、固定源码提交与对应源代码获取说明；
- 构建 commit、文件哈希和测试签名状态清单。

包内的“全部驱动”只指 PeerSpan 项目自己的虚拟显示驱动。不得捆绑或覆盖 NVIDIA/AMD/Intel 显卡驱动、Windows 自带 `WUDFRd.sys` 等系统组件；这些组件必须由目标机 Windows Update 或硬件厂商维护，避免安装不匹配版本。

当前驱动使用项目生成的测试证书，只允许安装到获得授权的专用测试机。正式发布前必须更换为微软认可的生产驱动签名，并对应用安装器做正式代码签名。

## 构建

在项目根目录执行：

```powershell
$env:RUSTUP_HOME = "D:\Dev\Env\Rust\rustup"
$env:CARGO_HOME = "D:\Dev\Env\Rust\cargo"
$env:PATH = "D:\Dev\Env\Rust\cargo\bin;$env:PATH"

npm run build:installer
```

首次构建需要下载 WebView2 Evergreen Standalone Installer，以及固定版本的 Sunshine/Moonlight 官方便携包。脚本把后两者缓存到 `D:\Dev\Env\PeerSpan\downloads` 并强制校验 SHA-256。直连失败时只在当前终端设置代理：

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

清单必须满足 `gitDirty: false`，并在复制到测试机前重新计算 SHA-256：

```powershell
Get-FileHash target\release\bundle\nsis\PeerSpan_0.1.0_x64-setup.exe -Algorithm SHA256
Get-Content -Raw target\release\bundle\nsis\PeerSpan_0.1.0_x64-setup.exe.manifest.json
```

## 安装、重启与卸载

1. 把单个 `PeerSpan_0.1.0_x64-setup.exe` 复制到目标机。
2. 双击运行并接受 UAC；安装器会明确提示这是内部测试包。
3. 安装器按机器安装应用与 Sunshine/Moonlight 核心，静默补齐 WebView2，信任测试证书，暂存并安装 PeerSpan 驱动，然后为 PeerSpan 和 Sunshine 添加 Domain/Private + LocalSubnet 防火墙规则。
4. 若 Windows 报告驱动或显示栈需要重启，应先关闭工作并重启，再通过物理控制台或不占用 Windows 显示拓扑的管理链路启用虚拟屏。
5. 从 Windows“应用和功能”卸载 PeerSpan；卸载钩子同时移除 PeerSpan 驱动包、对应测试证书和四条防火墙规则，Tauri 随应用目录删除两个串流核心。

活动 RDP 会话会加载 `Microsoft Remote Display Adapter` / `RdpIdd_IndirectDisplay`。Windows 可能因此以 `PNP_VetoOutstandingOpen` 拒绝显示栈重启；此时驱动可以已成功安装和启动，但新显示器仍不会进入桌面拓扑。不要把这种状态记录为虚拟屏验收通过。

## 192.168.9.26 安装链路记录

2026-08-01 已通过活动 RDP 在 `192.168.9.26` 完成一次全新安装验证。该次验证使用包含 PeerSpan 应用、驱动、测试证书和防火墙脚本的联网运行时包：

```text
SHA-256: 9996C4119606A5749E7952D5BAA8A6200032E359D73204EEEB0C9834C52EE6C6
source commit: deb6b02e76e1810eee161825ed604e7bae8b6283
source dirty: true
```

已确认：

- 应用安装到 `C:\Program Files\PeerSpan\peerspan-desktop.exe`，安装/卸载注册项存在；
- TCP `37621`、`37622` 监听且本机连接成功；
- 驱动包为 `oem9.inf`，Original Name 为 `PeerSpanIdd.inf`，Provider 为 `PeerSpan Project`；
- 测试证书指纹 `9AADD10ED9B76B3934414EA36E4B1FEDCF701706` 同时存在于 `LocalMachine\Root` 和 `TrustedPublisher`；
- `PeerSpan-LAN-TCP` 与 `PeerSpan-LAN-UDP` 仅允许 Domain/Private、Inbound、LocalSubnet；
- mDNS 已发现开发机 `HOME-PC`；
- SetupAPI 记录软件设备配置完成且设备节点 `Start` 返回 `SUCCESS`。

尚未通过：活动 RDP 的 `RdpIdd_IndirectDisplay` 对显示栈重启发出 `PNP_VetoOutstandingOpen`，Windows 标记 `Device required reboot`。当时版本因桌面拓扑中未出现 PeerSpan 显示器而释放设备，之后显示为 `CM_PROB_PHANTOM`；当前版本已改为在布局失败时保留设备租约并允许稍后重试，但仍必须重启后继续虚拟屏、真实串流、输入、剪贴板、恢复及卸载验收。

这次 SHA-256 仅用于保留安装链路证据，不作为最终交付包哈希；最终离线包必须由干净 commit 重新生成并以相邻 manifest 为准。
