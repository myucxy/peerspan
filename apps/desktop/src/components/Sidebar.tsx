import { AppWindow, LayoutDashboard, MonitorUp, Settings2 } from "lucide-react";
import type { LocalDevice, ViewKey } from "../types";
import { BrandMark } from "./BrandMark";

const navigation: Array<{ key: ViewKey; label: string; caption: string; icon: typeof LayoutDashboard }> = [
  { key: "home", label: "设备", caption: "发现与连接", icon: LayoutDashboard },
  { key: "display", label: "屏幕会话", caption: "布局与画质", icon: MonitorUp },
  { key: "nodes", label: "应用节点", caption: "后续里程碑", icon: AppWindow },
  { key: "settings", label: "设置", caption: "偏好与诊断", icon: Settings2 },
];

interface SidebarProps {
  active: ViewKey;
  localDevice?: LocalDevice;
  onNavigate: (key: ViewKey) => void;
}

export function Sidebar({ active, localDevice, onNavigate }: SidebarProps) {
  return (
    <aside className="sidebar">
      <div className="brand-lockup">
        <BrandMark />
        <div>
          <strong>PeerSpan</strong>
          <span>邻屏</span>
        </div>
      </div>

      <nav className="nav-list" aria-label="主导航">
        {navigation.map(({ key, label, caption, icon: Icon }) => (
          <button
            type="button"
            key={key}
            className={`nav-item ${active === key ? "nav-item-active" : ""}`}
            onClick={() => onNavigate(key)}
          >
            <span className="nav-icon"><Icon size={19} strokeWidth={1.8} /></span>
            <span className="nav-copy"><strong>{label}</strong><small>{caption}</small></span>
            {key === "nodes" && <span className="nav-badge">预览</span>}
          </button>
        ))}
      </nav>

      <div className="sidebar-spacer" />
      <div className="local-device-card">
        <span className="device-avatar local-avatar">{localDevice?.name.slice(0, 1) ?? "P"}</span>
        <div className="local-device-copy">
          <strong>{localDevice?.name ?? "正在识别此电脑"}</strong>
          <span><i className="online-dot" />本机服务在线</span>
        </div>
      </div>
      <p className="tagline">窗口跨屏，算力留在原机。</p>
    </aside>
  );
}
