import { ArrowRight, Cable, Clock3, Laptop, Monitor, Radar, RefreshCw, ShieldCheck, Wifi } from "lucide-react";
import type { AppSnapshot, PeerDevice } from "../types";
import { EmptyState } from "../components/EmptyState";

interface HomeViewProps {
  snapshot: AppSnapshot;
  scanning: boolean;
  preview: boolean;
  onScan: () => void;
  onSelectDevice: (device: PeerDevice) => void;
  onOpenDisplay: () => void;
  onCreatePairingOffer: () => void;
}

function DeviceCard({ device, onSelect }: { device: PeerDevice; onSelect: () => void }) {
  const connectionIcon = device.platform.includes("Wi-Fi") ? <Wifi size={14} /> : <Cable size={14} />;
  return (
    <article className="device-card">
      <div className="device-card-top">
        <span className={`device-avatar ${device.trusted ? "trusted-avatar" : ""}`}><Laptop size={23} /></span>
        <span className={`status-chip status-${device.status}`}><i />{device.status === "online" ? "可连接" : device.status === "busy" ? "使用中" : "离线"}</span>
      </div>
      <div className="device-card-copy">
        <h3>{device.name}</h3>
        <p>{device.platform}</p>
      </div>
      <div className="device-metadata">
        <span>{connectionIcon}{device.latencyMs ? `${device.latencyMs} ms` : "等待响应"}</span>
        <span>{device.trusted ? <><ShieldCheck size={14} />已信任</> : <><Clock3 size={14} />待配对</>}</span>
      </div>
      <button className={device.trusted ? "primary-button" : "secondary-button"} type="button" onClick={onSelect} disabled={device.status !== "online"}>
        {device.trusted ? "用作扩展屏" : "安全配对"}<ArrowRight size={16} />
      </button>
    </article>
  );
}

export function HomeView({ snapshot, scanning, preview, onScan, onSelectDevice, onOpenDisplay, onCreatePairingOffer }: HomeViewProps) {
  const devices = [
    ...snapshot.nearbyDevices,
    ...snapshot.trustedDevices.filter((trusted) => !snapshot.nearbyDevices.some((nearby) => nearby.id === trusted.id)),
  ];
  return (
    <div className="view-shell home-view">
      {preview && <div className="preview-banner"><span>设计预览</span>当前设备为界面样例，桌面版不会注入模拟节点。</div>}

      <section className="hero-card">
        <div className="hero-copy">
          <p className="eyebrow">局域网桌面扩展</p>
          <h1>让身边的电脑，<br /><span>成为你的下一块屏幕。</span></h1>
          <p>应用仍在原电脑运行。PeerSpan 只传递画面与输入，让两台 Windows 电脑自然协作。</p>
          <div className="hero-actions">
            <button className="primary-button large-button" type="button" onClick={onScan} disabled={scanning}>
              {scanning ? <RefreshCw className="spin" size={17} /> : <Radar size={17} />}
              {scanning ? "正在发现设备" : "发现附近设备"}
            </button>
            <button className="text-button" type="button" onClick={onOpenDisplay}>调整屏幕布局<ArrowRight size={15} /></button>
            <button className="text-button" type="button" onClick={onCreatePairingOffer}><ShieldCheck size={14} />本机配对码</button>
          </div>
        </div>
        <div className="hero-visual" aria-hidden="true">
          <div className="ambient-orbit orbit-one" />
          <div className="ambient-orbit orbit-two" />
          <div className="screen-illustration screen-source">
            <span className="screen-camera" /><div className="screen-window"><i /><i /><i /><b /></div><em>此电脑</em>
          </div>
          <div className="stream-line"><i /><i /><i /></div>
          <div className="screen-illustration screen-target">
            <span className="screen-camera" /><div className="screen-window remote-window"><i /><i /><i /><b /></div><em>扩展屏</em>
          </div>
        </div>
      </section>

      <section className="section-block">
        <div className="section-heading">
          <div><p className="eyebrow">附近设备</p><h2>选择一台电脑开始</h2></div>
          <button className="icon-text-button" type="button" onClick={onScan} disabled={scanning}><RefreshCw className={scanning ? "spin" : ""} size={15} />刷新</button>
        </div>
        {devices.length > 0 ? (
          <div className="device-grid">
            {devices.map((device) => <DeviceCard key={device.id} device={device} onSelect={() => onSelectDevice(device)} />)}
          </div>
        ) : (
          <EmptyState icon={Monitor} title="暂未发现其他电脑" detail="请确认两台电脑位于同一局域网，并在对方电脑打开 PeerSpan。" action={<button className="secondary-button" type="button" onClick={onScan}>重新扫描</button>} />
        )}
      </section>
    </div>
  );
}
