import { Check, ChevronDown, CircleStop, Info, Keyboard, LoaderCircle, Monitor, MonitorOff, MonitorUp, MousePointer2, RotateCw, Sparkles } from "lucide-react";
import type { AppSnapshot, Preferences, ScreenEdge } from "../types";

interface DisplayViewProps {
  snapshot: AppSnapshot;
  virtualDisplayBusy: boolean;
  onChangePreferences: (preferences: Preferences) => Promise<void>;
  onSetVirtualDisplay: (enabled: boolean) => Promise<void>;
  onEndSession: (sessionId: string) => Promise<void>;
}

const edges: Array<{ value: ScreenEdge; label: string }> = [
  { value: "left", label: "左侧" },
  { value: "right", label: "右侧" },
  { value: "top", label: "上方" },
  { value: "bottom", label: "下方" },
];

const qualityLabels = { clarity: "清晰优先 · 20 Mbps", balanced: "平衡 · 12 Mbps", responsive: "响应优先 · 8 Mbps" } as const;

export function DisplayView({ snapshot, virtualDisplayBusy, onChangePreferences, onSetVirtualDisplay, onEndSession }: DisplayViewProps) {
  const { preferences } = snapshot;
  const updateEdge = (screenEdge: ScreenEdge) => void onChangePreferences({ ...preferences, screenEdge });
  const driverReady = snapshot.capabilities.virtualDisplay.state === "ready";
  const mediaReady = snapshot.capabilities.mediaPipeline.state === "ready";
  const inputReady = snapshot.capabilities.inputInjection.state === "ready";
  const nativeLinkReady = driverReady && mediaReady && inputReady;
  const session = snapshot.activeSession;
  const readiness = session
    ? session.state === "streaming" ? "正在串流" : session.state === "recovering" ? "正在恢复" : "认证会话协商中"
    : nativeLinkReady ? "可以建立会话" : driverReady ? "虚拟屏已启用" : "虚拟屏未启用";

  return (
    <div className="view-shell display-view">
      <div className="page-heading">
        <div><p className="eyebrow">扩展屏模式</p><h1>屏幕会话</h1><p>定义远端屏幕的位置、分辨率和输入行为。</p></div>
        <div className="display-heading-actions">
          <span className={`readiness-pill ${driverReady || session ? "is-ready" : ""}`}><i />{readiness}</span>
          {session && <button className="secondary-button" type="button" onClick={() => void onEndSession(session.id)}><CircleStop size={15} />结束会话</button>}
          {!session && <button className={driverReady ? "secondary-button" : "primary-button"} type="button" disabled={virtualDisplayBusy} onClick={() => void onSetVirtualDisplay(!driverReady)}>{virtualDisplayBusy ? <LoaderCircle className="spin" size={15} /> : driverReady ? <MonitorOff size={15} /> : <MonitorUp size={15} />}{driverReady ? "撤销虚拟屏" : "启用虚拟屏"}</button>}
        </div>
      </div>

      <section className="layout-card">
        <div className="layout-toolbar">
          <div><h2>屏幕布局</h2><p>窗口越过所选边缘后，将进入 PeerSpan 虚拟屏。</p></div>
          <button className="select-button" type="button"><Monitor size={15} />1920 × 1080 · 60 Hz<ChevronDown size={15} /></button>
        </div>
        <div className={`display-stage edge-${preferences.screenEdge}`}>
          <div className="stage-grid" />
          <div className="monitor-tile local-monitor"><span className="monitor-number">1</span><div><strong>此电脑</strong><small>主显示器 · 100%</small></div></div>
          <div className="edge-connector"><i /><span>窗口从这里跨越</span></div>
          <div className="monitor-tile remote-monitor"><span className="monitor-number">2</span><div><strong>PeerSpan 虚拟屏</strong><small>1920 × 1080 · 60 Hz</small></div><span className="remote-glow" /></div>
        </div>
        <div className="edge-picker" role="group" aria-label="虚拟屏方向">
          <span>虚拟屏位于主屏</span>
          {edges.map((edge) => (
            <button type="button" className={preferences.screenEdge === edge.value ? "selected" : ""} key={edge.value} onClick={() => updateEdge(edge.value)}>
              {preferences.screenEdge === edge.value && <Check size={14} />}{edge.label}
            </button>
          ))}
        </div>
      </section>

      <div className="two-column-grid">
        <section className="settings-card">
          <div className="card-title-row"><span className="soft-icon"><Sparkles size={18} /></span><div><h3>画面策略</h3><p>连接时自动协商，无需手工匹配缩放。</p></div></div>
          <dl className="detail-list">
            <div><dt>清晰度</dt><dd>{qualityLabels[preferences.quality]}</dd></div>
            <div><dt>DPI 感知</dt><dd>Per-Monitor V2</dd></div>
            <div><dt>旋转适配</dt><dd><RotateCw size={14} />自动</dd></div>
          </dl>
        </section>
        <section className="settings-card">
          <div className="card-title-row"><span className="soft-icon amber"><MousePointer2 size={18} /></span><div><h3>输入控制</h3><p>断线时自动释放，不持续占用本机输入。</p></div></div>
          <dl className="detail-list">
            <div><dt>键鼠回传</dt><dd><Keyboard size={14} />认证后启用</dd></div>
            <div><dt>紧急释放</dt><dd className="shortcut-value">{preferences.releaseShortcut}</dd></div>
            <div><dt>断线释放目标</dt><dd>1 秒以内</dd></div>
          </dl>
        </section>
      </div>

      {!driverReady && <div className="info-callout"><Info size={18} /><div><strong>虚拟显示器尚未启用</strong><p>“启用虚拟屏”会调用 Windows 软件设备 API，并且只有 IddCx 设备节点真实启动后才会显示就绪。当前诊断：{snapshot.capabilities.virtualDisplay.detail}</p></div></div>}
      {driverReady && !nativeLinkReady && <div className="info-callout"><Info size={18} /><div><strong>IddCx 虚拟显示器已经真实启动</strong><p>设备会在撤销或 PeerSpan 退出时移除。媒体编码与输入链路尚未同时就绪，因此目前不会把认证控制会话标记为正在串流。</p></div></div>}
      {session && <div className="info-callout session-callout"><Info size={18} /><div><strong>TLS 1.3 控制通道已认证</strong><p>会话 {session.id.slice(0, 8)} 正在协商 {session.widthPx} × {session.heightPx} · {session.refreshHz} Hz；只有媒体管线确认就绪后才会进入“正在串流”。</p></div></div>}
    </div>
  );
}
