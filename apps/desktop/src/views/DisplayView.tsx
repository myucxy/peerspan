import { Check, ChevronDown, Info, Keyboard, Monitor, MousePointer2, RotateCw, Sparkles } from "lucide-react";
import type { AppSnapshot, Preferences, ScreenEdge } from "../types";

interface DisplayViewProps {
  snapshot: AppSnapshot;
  onChangePreferences: (preferences: Preferences) => Promise<void>;
}

const edges: Array<{ value: ScreenEdge; label: string }> = [
  { value: "left", label: "左侧" },
  { value: "right", label: "右侧" },
  { value: "top", label: "上方" },
  { value: "bottom", label: "下方" },
];

export function DisplayView({ snapshot, onChangePreferences }: DisplayViewProps) {
  const { preferences } = snapshot;
  const updateEdge = (screenEdge: ScreenEdge) => void onChangePreferences({ ...preferences, screenEdge });
  const driverReady = snapshot.capabilities.virtualDisplay.state === "ready";

  return (
    <div className="view-shell display-view">
      <div className="page-heading">
        <div><p className="eyebrow">扩展屏模式</p><h1>屏幕会话</h1><p>定义远端屏幕的位置、分辨率和输入行为。</p></div>
        <span className={`readiness-pill ${driverReady ? "is-ready" : ""}`}><i />{driverReady ? "可以建立会话" : "等待虚拟显示驱动"}</span>
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
            <div><dt>清晰度</dt><dd>平衡模式</dd></div>
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

      {!driverReady && <div className="info-callout"><Info size={18} /><div><strong>原生图形链路尚未接入</strong><p>当前已经固定 UI、配置和协议边界。安装并验证 IddCx 驱动前，不会提供一个看似成功但没有画面的“开始会话”按钮。</p></div></div>}
    </div>
  );
}
