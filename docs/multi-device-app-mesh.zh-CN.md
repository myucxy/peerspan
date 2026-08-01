# 多设备与应用节点设计

更新日期：2026-08-02

## 需求结论

PeerSpan 的互联关系按对等节点处理：一台电脑可同时与多台受信电脑保持屏幕会话；任意一次认证连接都会双向交换双方当前可发布的应用目录。应用节点页按机器分组，屏幕页按设备保存自由拖动布局。

本阶段完成“发现和发布哪些应用可用”，不越权实现远程启动。远程执行仍需要独立的授权、审计、用户会话 Agent 与进程生命周期协议，不能仅凭目录同步直接执行远端命令。

## 状态模型

- `displaySessions` 是活动会话集合，以 session UUID 唯一标识。
- 同一 peer 最多一条活动屏幕会话，不同 peer 可并存；全局安全上限为 8。
- `displayLayouts` 以 peer UUID 保存 x/y 坐标，持久化后不因其他 peer 掉线而改变。
- `localApplications` 保存本机私有启动信息。
- `applicationCatalogs` 保存已认证远端节点发布的安全摘要，按 device UUID 替换或升级 revision。

## 协议 v6

TLS 1.3 与 Ed25519 固定身份验证保持不变，ALPN 升级为 `peerspan-control/6`。Hello 完成后必须先交换 `ApplicationCatalogExchange`：

1. 发起方发送本机启用应用的 UUID、名称、类型及 revision。
2. 接收方验证 catalog 的 device UUID 与证书/Hello 对应的受信 peer 一致。
3. 接收方保存目录并返回自己的目录。
4. `continueControl=false` 时连接结束；为 `true` 时继续屏幕 Offer/Decision。

协议不发送启动路径、参数、工作目录或图标文件；目录最多 512 项，名称最多 128 字符，控制帧仍受 8 MiB 总上限保护。

## Windows 应用来源

- 手工：用户添加 EXE、命令或 `.lnk`，可编辑、启停和删除。
- 自动：扫描 `%APPDATA%\Microsoft\Windows\Start Menu\Programs` 与 `%PROGRAMDATA%\Microsoft\Windows\Start Menu\Programs`。
- 扫描路径规范化后用 UUID v5 生成稳定 ID，过滤卸载、Readme、发行说明等维护快捷方式。
- 手工记录优先于同 ID 扫描结果；再次扫描保留已有扫描项的启停状态。

本阶段没有扫描注册表卸载项或 AppX 清单，因为开始菜单快捷方式是 Windows 用户实际可启动入口，并可直接由 Shell 处理。后续若加入 AppX，仍必须合并到相同的私有/安全摘要边界。

## 会话与故障隔离

控制信号、原生媒体端点、Moonlight worker 和结束标志都以 session UUID 存入映射。自动重连使用待重连 peer 集合，各 peer 独立限流；某一 session 超时、主动结束或媒体失败只删除自己的状态。Sunshine 主机进程可以共享，但单一客户端失败不能停止主机或其他会话。

## 验证

- 核心测试：不同 peer 的并发会话、同 peer 防重、状态迁移、布局重启持久化。
- 协议测试：目录 JSON 往返且不出现启动路径/参数。
- 桌面集成测试：三个本地身份、三个 TCP 监听器与双向 TLS；中心节点同步两个目录、保持两条会话、结束第一条后第二条继续存在。
- UI：1983×1067 视口检查空布局、双节点拖拽、三机器应用分组和添加表单；控制台无错误或警告。
- 待外部验收：在 `192.168.9.26` 安装同一 v6 构建，验证双机真实画面、RDP 退出后的 VDD 拓扑、并发 Moonlight 实例、断网恢复及长时间运行。
