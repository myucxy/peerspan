import { CheckCircle2, CircleDashed, Clipboard, Info, Keyboard, MonitorCog, Network, Shield, Wrench } from "lucide-react";
import { useEffect, useState } from "react";
import type { AppSnapshot, Capability, Preferences, QualityMode } from "../types";

interface SettingsViewProps {
  snapshot: AppSnapshot;
  onChangePreferences: (preferences: Preferences) => Promise<void>;
}

function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (value: boolean) => void; label: string }) {
  return <button type="button" role="switch" aria-checked={checked} aria-label={label} className={`toggle ${checked ? "toggle-on" : ""}`} onClick={() => onChange(!checked)}><span /></button>;
}

function CapabilityRow({ name, capability }: { name: string; capability: Capability }) {
  const ready = capability.state === "ready";
  return <div className="capability-row"><span className={`capability-icon ${ready ? "ready" : ""}`}>{ready ? <CheckCircle2 size={17} /> : <CircleDashed size={17} />}</span><div><strong>{name}</strong><small>{capability.detail}</small></div><em>{ready ? "就绪" : capability.state === "requiresSetup" ? "需安装" : "待实现"}</em></div>;
}

export function SettingsView({ snapshot, onChangePreferences }: SettingsViewProps) {
  const p = snapshot.preferences;
  const [shortcut, setShortcut] = useState(p.releaseShortcut);
  useEffect(() => setShortcut(p.releaseShortcut), [p.releaseShortcut]);
  const update = (change: Partial<Preferences>) => void onChangePreferences({ ...p, ...change });
  const saveShortcut = () => {
    if (shortcut !== p.releaseShortcut) update({ releaseShortcut: shortcut });
  };
  const qualities: Array<{ value: QualityMode; label: string; detail: string }> = [
    { value: "clarity", label: "清晰优先", detail: "1080p60 · 20 Mbps" },
    { value: "balanced", label: "平衡", detail: "1080p60 · 12 Mbps" },
    { value: "responsive", label: "响应优先", detail: "1080p60 · 8 Mbps" },
  ];

  return (
    <div className="view-shell settings-view">
      <div className="page-heading"><div><p className="eyebrow">Preferences</p><h1>设置与诊断</h1><p>控制连接行为，并查看当前原生能力是否就绪。</p></div></div>

      <div className="settings-layout">
        <div className="settings-stack">
          <section className="preference-card">
            <div className="preference-heading"><span className="soft-icon"><MonitorCog size={19} /></span><div><h2>连接</h2><p>管理启动与恢复策略</p></div></div>
            <div className="preference-row"><div><strong>随 Windows 启动</strong><small>登录后自动启动 PeerSpan</small></div><Toggle label="随 Windows 启动" checked={p.launchAtStartup} onChange={(launchAtStartup) => update({ launchAtStartup })} /></div>
            <div className="preference-row"><div><strong>自动恢复会话</strong><small>短暂断网或睡眠唤醒后尝试重新连接</small></div><Toggle label="自动恢复会话" checked={p.autoReconnect} onChange={(autoReconnect) => update({ autoReconnect })} /></div>
            <div className="preference-row"><div><strong>文本剪贴板同步</strong><small>只同步文本；文件拖放不在 MVP 范围</small></div><Toggle label="文本剪贴板同步" checked={p.clipboardSync} onChange={(clipboardSync) => update({ clipboardSync })} /></div>
          </section>

          <section className="preference-card">
            <div className="preference-heading"><span className="soft-icon amber"><Network size={19} /></span><div><h2>画质策略</h2><p>输入坐标始终独立于画面降采样</p></div></div>
            <div className="quality-options">
              {qualities.map((quality) => <button type="button" key={quality.value} className={p.quality === quality.value ? "selected" : ""} onClick={() => update({ quality: quality.value })}><span>{p.quality === quality.value && <CheckCircle2 size={15} />}</span><div><strong>{quality.label}</strong><small>{quality.detail}</small></div></button>)}
            </div>
          </section>

          <section className="preference-card compact-card">
            <div className="preference-heading"><span className="soft-icon slate"><Keyboard size={19} /></span><div><h2>紧急释放快捷键</h2><p>任何时候都应能立即取回本机输入</p></div></div>
            <div className="shortcut-editor"><input aria-label="紧急释放快捷键" value={shortcut} onChange={(event) => setShortcut(event.target.value)} onBlur={saveShortcut} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }} /><button type="button" onClick={() => { setShortcut("Ctrl+Alt+Shift+Esc"); update({ releaseShortcut: "Ctrl+Alt+Shift+Esc" }); }}>恢复默认</button></div>
          </section>
        </div>

        <aside className="diagnostics-card">
          <div className="diagnostics-heading"><span><Wrench size={18} /></span><div><h2>运行能力</h2><p>当前电脑</p></div></div>
          <CapabilityRow name="控制桥接" capability={snapshot.capabilities.controlBridge} />
          <CapabilityRow name="局域网发现" capability={snapshot.capabilities.discovery} />
          <CapabilityRow name="安全配对" capability={snapshot.capabilities.securePairing} />
          <CapabilityRow name="认证控制通道" capability={snapshot.capabilities.secureControl} />
          <CapabilityRow name="虚拟显示器" capability={snapshot.capabilities.virtualDisplay} />
          <CapabilityRow name="媒体管线" capability={snapshot.capabilities.mediaPipeline} />
          <CapabilityRow name="输入注入" capability={snapshot.capabilities.inputInjection} />
          <div className="fingerprint-box"><Shield size={16} /><div><small>本机身份指纹</small><code>{snapshot.localDevice.fingerprint}</code></div></div>
          <div className="scope-note"><Info size={15} /><span>诊断状态来自桌面后端；浏览器设计预览仅用于检查界面。</span></div>
        </aside>
      </div>
      <div className="settings-footer-note"><Clipboard size={15} />MVP 已统一为“文本剪贴板”，图片与文件传输将在独立权限模型完成后评估。</div>
    </div>
  );
}
