# PeerSpan Windows 专用机验收手册

更新日期：2026-08-01

本文用于完成普通开发机无法安全执行的最后验收：安装 PeerSpan 测试/正式签名 IddCx 驱动，在两台真实 Windows 电脑之间验证完整 1080p60 链路。验收不得使用模拟画面，也不得仅凭控制通道连接就记录为串流成功。

## 测试矩阵

至少覆盖：

| 来源电脑 | 接收电脑 | 网络 | 必测项 |
| --- | --- | --- | --- |
| Windows 10 22H2 build 19045 | Windows 11 当前稳定版 | 千兆有线 | 驱动枚举、60 Hz、媒体、输入、剪贴板、断网 |
| Windows 10 Enterprise LTSC 2021 build 19044 | Windows 10 22H2 | Wi-Fi 6 | 睡眠恢复、DPI 125%/150%、自动重连 |
| Windows 11 当前稳定版 | Windows 10 22H2 | 千兆有线或 Wi-Fi 6 | 角色反转、卸载清理 |

每台机器记录 Windows build、GPU/驱动版本、连接类型、缩放比例、PeerSpan commit 和驱动包哈希。

## 1. 构建与驱动安装

在来源电脑的非管理员终端构建：

```powershell
$env:RUSTUP_HOME = "D:\Dev\Env\Rust\rustup"
$env:CARGO_HOME = "D:\Dev\Env\Rust\cargo"
$env:PATH = "D:\Dev\Env\Rust\cargo\bin;$env:PATH"

npm run build
pwsh -File native\idd\build.ps1 -Configuration Release -Platform x64
```

确认构建输出没有 InfVerif、Catalog 或签名警告。测试证书信任和驱动安装属于系统变更，只能由测试机管理员审阅脚本后，在提升权限的 PowerShell 中显式执行：

```powershell
pwsh -File native\idd\install-dev.ps1 `
  -Configuration Release `
  -Platform x64 `
  -TrustTestCertificate `
  -AcknowledgeSystemChanges `
  -Confirm
```

生产签名包不使用 `-TrustTestCertificate`。安装后用设备管理器或 `pnputil /enum-drivers` 确认 Provider 为 `PeerSpan Project`，不得开启全局忽略驱动签名策略来掩盖包错误。

## 2. 虚拟显示器

1. 启动 `target\release\peerspan-desktop.exe`。
2. 在“屏幕会话”页启用虚拟屏。
3. 只有界面显示 IddCx 设备节点 `DN_STARTED` 后继续。
4. 在 Windows 显示设置中确认 `PeerSpan Virtual Display` 为扩展模式、1920×1080、60 Hz。
5. 依次选择左、右、上、下，确认 Windows 真实显示拓扑随界面设置变化。
6. 活动会话期间尝试撤销虚拟屏，必须被拒绝；结束会话后撤销必须移除设备节点。

## 3. 双机媒体会话

1. 两机完成 mDNS 发现和六位代码配对，核对双方身份指纹。
2. 来源电脑启用虚拟屏并选择已配对接收电脑。
3. 接收电脑必须出现原生 PeerSpan 窗口；在首个真实硬解帧 `Present` 前，两端状态只能是“协商中”。
4. 把动态窗口、滚动页面和视频拖到虚拟屏，确认远端内容与来源一致且没有测试图或重复旧帧。
5. 记录 10 分钟和 8 小时运行结果；观察闪屏、花屏、黑屏、内存/GPU 占用持续增长和会话资源泄漏。
6. 用 240 fps 高速摄像或等价外部测量同时拍摄来源输入动作与接收画面，计算 P50/P95 端到端延迟；目标为稳定局域网 P95 小于 80 ms。

PeerSpan 内部控制 RTT 只能辅助诊断，不能替代玻璃到玻璃延迟测量。

## 4. 输入与剪贴板

1. 在接收窗口验证绝对鼠标、左右/中/前进/后退按钮、垂直/水平滚轮。
2. 验证普通键、组合键、扩展扫描码、长按去重和输入法切换；输入只能落到来源虚拟屏上的窗口。
3. 按设置中的紧急释放组合，确认立即产生 Release All，来源电脑没有残留按下的修饰键或鼠标键。
4. 接收窗口失焦、关闭和网线拔出时重复验证全键释放。
5. 双向复制 ASCII、中文、emoji、多行文本、空文本和接近 1 MiB 的文本；确认不循环、不传图片/富文本/文件，超限文本不发送。

## 5. 异常恢复

每种异常至少重复 10 次：

- 拔网线或关闭 Wi-Fi，再恢复；
- 来源电脑和接收电脑分别睡眠/唤醒；
- 会话中切换 DPI 100%/125%/150%/200%；
- 会话中触发显示模式重建或 GPU 驱动重启；
- 关闭接收窗口、结束来源会话、退出任一 PeerSpan 进程。

预期结果：1 秒内释放注入输入；媒体、解码器、交换链和控制通道退出；来源虚拟屏上的普通窗口回迁主屏；启用“自动恢复会话”时，mDNS 重新发现设备后持续重试并恢复到首帧确认的 `Streaming`。

## 6. 卸载与证据

测试完成后，在提升权限的 PowerShell 中显式卸载测试包和测试证书：

```powershell
pwsh -File native\idd\uninstall-dev.ps1 `
  -Configuration Release `
  -Platform x64 `
  -RemoveTestCertificate `
  -AcknowledgeSystemChanges `
  -Confirm
```

保存每个矩阵项的构建日志、驱动枚举、两端截图/录像、延迟原始数据、异常恢复次数和失败复现步骤。只有矩阵全部通过后，才能把需求状态中的 IddCx 与真实跨机最后一跳标记为“已完成”。
