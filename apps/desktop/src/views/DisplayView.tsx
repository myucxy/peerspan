import { Check, CircleStop, Grip, Info, Keyboard, LoaderCircle, Monitor, MonitorOff, MonitorUp, MousePointer2, RotateCw, Sparkles, Wifi } from "lucide-react";
import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import type { AppSnapshot, DisplaySession, Preferences, ScreenEdge } from "../types";

interface DisplayViewProps {
  snapshot: AppSnapshot;
  virtualDisplayBusy: boolean;
  onChangePreferences: (preferences: Preferences) => Promise<void>;
  onSetVirtualDisplay: (enabled: boolean) => Promise<void>;
  onSetLayout: (peerId: string, x: number, y: number) => Promise<void>;
  onEndSession: (sessionId: string) => Promise<void>;
}

interface Position { x: number; y: number }
interface DragState { peerId: string; offsetX: number; offsetY: number; latest?: Position }

const edges: Array<{ value: ScreenEdge; label: string }> = [
  { value: "left", label: "左侧" },
  { value: "right", label: "右侧" },
  { value: "top", label: "上方" },
  { value: "bottom", label: "下方" },
];

const qualityLabels = { clarity: "清晰优先 · 20 Mbps", balanced: "平衡 · 12 Mbps", responsive: "响应优先 · 8 Mbps" } as const;

export function DisplayView({ snapshot, virtualDisplayBusy, onChangePreferences, onSetVirtualDisplay, onSetLayout, onEndSession }: DisplayViewProps) {
  const { preferences, displaySessions } = snapshot;
  const stageRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | undefined>(undefined);
  const [positions, setPositions] = useState<Record<string, Position>>({});
  const driverReady = snapshot.capabilities.virtualDisplay.state === "ready";
  const streamingCount = displaySessions.filter((session) => session.state === "streaming").length;
  const readiness = displaySessions.length > 0
    ? `${streamingCount}/${displaySessions.length} 个会话正在串流`
    : driverReady ? "虚拟屏已就绪，可同时连接多台设备" : "虚拟屏未启用";

  useEffect(() => {
    setPositions((current) => {
      const next = { ...current };
      displaySessions.forEach((session, index) => {
        const persisted = snapshot.displayLayouts.find((layout) => layout.peerId === session.peerId);
        if (persisted) next[session.peerId] = { x: persisted.x, y: persisted.y };
        else if (!next[session.peerId]) next[session.peerId] = { x: 330 + (index % 3) * 194, y: 22 + Math.floor(index / 3) * 112 };
      });
      return next;
    });
  }, [displaySessions, snapshot.displayLayouts]);

  const peerName = (session: DisplaySession) => snapshot.nearbyDevices.find((peer) => peer.id === session.peerId)?.name
    ?? snapshot.trustedDevices.find((peer) => peer.id === session.peerId)?.name
    ?? `设备 ${session.peerId.slice(0, 6)}`;

  const beginDrag = (event: ReactPointerEvent<HTMLDivElement>, peerId: string) => {
    const tile = event.currentTarget.getBoundingClientRect();
    dragRef.current = { peerId, offsetX: event.clientX - tile.left, offsetY: event.clientY - tile.top };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const moveDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    const stage = stageRef.current?.getBoundingClientRect();
    if (!drag || !stage) return;
    const x = Math.round(Math.max(8, Math.min(stage.width - 190, event.clientX - stage.left - drag.offsetX)));
    const y = Math.round(Math.max(8, Math.min(stage.height - 108, event.clientY - stage.top - drag.offsetY)));
    drag.latest = { x, y };
    setPositions((current) => ({ ...current, [drag.peerId]: { x, y } }));
  };

  const finishDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    dragRef.current = undefined;
    if (!drag) return;
    event.currentTarget.releasePointerCapture(event.pointerId);
    const position = drag.latest ?? positions[drag.peerId];
    if (position) void onSetLayout(drag.peerId, position.x, position.y);
  };

  const arrange = (screenEdge: ScreenEdge) => {
    void onChangePreferences({ ...preferences, screenEdge });
    displaySessions.forEach((session, index) => {
      const horizontal = screenEdge === "left" || screenEdge === "right";
      const position = horizontal
        ? { x: screenEdge === "left" ? 18 : 485, y: 18 + index * 112 }
        : { x: 245 + index * 194, y: screenEdge === "top" ? 12 : 168 };
      setPositions((current) => ({ ...current, [session.peerId]: position }));
      void onSetLayout(session.peerId, position.x, position.y);
    });
  };

  return (
    <div className="view-shell display-view">
      <div className="page-heading">
        <div><p className="eyebrow">多设备扩展屏</p><h1>屏幕会话</h1><p>每台设备拥有独立链路；拖动屏幕卡片即可保存空间布局。</p></div>
        <div className="display-heading-actions">
          <span className={`readiness-pill ${driverReady || displaySessions.length ? "is-ready" : ""}`}><i />{readiness}</span>
          {displaySessions.length === 0 && <button className={driverReady ? "secondary-button" : "primary-button"} type="button" disabled={virtualDisplayBusy} onClick={() => void onSetVirtualDisplay(!driverReady)}>{virtualDisplayBusy ? <LoaderCircle className="spin" size={15} /> : driverReady ? <MonitorOff size={15} /> : <MonitorUp size={15} />}{driverReady ? "撤销虚拟屏" : "启用虚拟屏"}</button>}
        </div>
      </div>

      <section className="layout-card">
        <div className="layout-toolbar">
          <div><h2>自由屏幕布局</h2><p>布局按设备保存，单台掉线不会重排其他屏幕。</p></div>
          <span className="layout-count"><Wifi size={14} />{displaySessions.length} 个活动节点</span>
        </div>
        <div className="display-stage multi-display-stage" ref={stageRef}>
          <div className="stage-grid" />
          <div className="monitor-tile local-monitor multi-local-monitor"><span className="monitor-number">1</span><div><strong>此电脑</strong><small>本机桌面 · VDD 源</small></div></div>
          {displaySessions.map((session, index) => {
            const position = positions[session.peerId] ?? { x: 330 + index * 194, y: 22 };
            return (
              <div
                className={`monitor-tile remote-monitor draggable-monitor state-${session.state}`}
                key={session.id}
                style={{ left: position.x, top: position.y }}
                onPointerDown={(event) => beginDrag(event, session.peerId)}
                onPointerMove={moveDrag}
                onPointerUp={finishDrag}
              >
                <Grip className="drag-grip" size={13} />
                <span className="monitor-number">{index + 2}</span>
                <div><strong>{peerName(session)}</strong><small>{session.widthPx} × {session.heightPx} · {session.latencyMs ?? "—"} ms</small></div>
                <button className="tile-stop" type="button" title="结束此会话" onPointerDown={(event) => event.stopPropagation()} onClick={() => void onEndSession(session.id)}><CircleStop size={13} /></button>
                <span className="remote-glow" />
              </div>
            );
          })}
          {displaySessions.length === 0 && <div className="layout-empty"><Monitor size={22} /><strong>尚无活动屏幕会话</strong><small>从“附近设备”可同时连接多台已信任电脑</small></div>}
        </div>
        <div className="edge-picker" role="group" aria-label="快速排列屏幕">
          <span>快速排列到本机</span>
          {edges.map((edge) => <button type="button" className={preferences.screenEdge === edge.value ? "selected" : ""} key={edge.value} onClick={() => arrange(edge.value)}>{preferences.screenEdge === edge.value && <Check size={14} />}{edge.label}</button>)}
          <em>也可直接拖动</em>
        </div>
      </section>

      <div className="two-column-grid">
        <section className="settings-card">
          <div className="card-title-row"><span className="soft-icon"><Sparkles size={18} /></span><div><h3>画面策略</h3><p>每个会话独立协商和恢复。</p></div></div>
          <dl className="detail-list"><div><dt>清晰度</dt><dd>{qualityLabels[preferences.quality]}</dd></div><div><dt>并发上限</dt><dd>8 台设备</dd></div><div><dt>旋转适配</dt><dd><RotateCw size={14} />自动</dd></div></dl>
        </section>
        <section className="settings-card">
          <div className="card-title-row"><span className="soft-icon amber"><MousePointer2 size={18} /></span><div><h3>输入与隔离</h3><p>连接按会话释放，故障不会扩散。</p></div></div>
          <dl className="detail-list"><div><dt>键鼠回传</dt><dd><Keyboard size={14} />认证后启用</dd></div><div><dt>紧急释放</dt><dd className="shortcut-value">{preferences.releaseShortcut}</dd></div><div><dt>失活判定</dt><dd>1 秒</dd></div></dl>
        </section>
      </div>

      {!driverReady && <div className="info-callout"><Info size={18} /><div><strong>虚拟显示器尚未启用</strong><p>启用后，同一块 VDD 扩展桌面可由 Sunshine 并发提供给多个受信节点；当前诊断：{snapshot.capabilities.virtualDisplay.detail}</p></div></div>}
      {displaySessions.length > 0 && <div className="info-callout session-callout"><Info size={18} /><div><strong>{displaySessions.length} 条 TLS 1.3 控制通道独立运行</strong><p>心跳、媒体进程、结束信号和自动重连均按会话隔离；结束某一设备不会清理其他设备。</p></div></div>}
    </div>
  );
}
