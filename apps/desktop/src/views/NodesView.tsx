import { AppWindow, Check, Command, Edit3, Laptop, LoaderCircle, Plus, Radar, RefreshCw, Search, Server, TerminalSquare, Trash2, X } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { removePublishedApplication, savePublishedApplication, scanPublishedApplications, syncApplicationCatalogs } from "../lib/bridge";
import type { AppSnapshot, ApplicationKind, PublishedApplication } from "../types";

interface NodesViewProps {
  snapshot: AppSnapshot;
  onSnapshotChanged: (snapshot: AppSnapshot) => void;
  onError: (message: string) => void;
  onSuccess: (message: string) => void;
}

interface ApplicationForm {
  id?: string;
  name: string;
  launchTarget: string;
  arguments: string;
  kind: ApplicationKind;
}

const emptyForm: ApplicationForm = { name: "", launchTarget: "", arguments: "", kind: "gui" };
const errorMessage = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);

export function NodesView({ snapshot, onSnapshotChanged, onError, onSuccess }: NodesViewProps) {
  const [busy, setBusy] = useState<"scan" | "sync" | "save" | string>();
  const [query, setQuery] = useState("");
  const [form, setForm] = useState<ApplicationForm>();
  const syncedOnce = useRef(false);

  useEffect(() => {
    if (syncedOnce.current) return;
    syncedOnce.current = true;
    setBusy("sync");
    syncApplicationCatalogs()
      .then(onSnapshotChanged)
      .catch((reason) => onError(errorMessage(reason)))
      .finally(() => setBusy(undefined));
  }, [onError, onSnapshotChanged]);

  const scan = async () => {
    setBusy("scan");
    try {
      const next = await scanPublishedApplications();
      onSnapshotChanged(next);
      onSuccess(`已从 Windows 开始菜单更新 ${next.localApplications.filter((app) => app.source === "startMenu").length} 个应用`);
    } catch (reason) { onError(errorMessage(reason)); }
    finally { setBusy(undefined); }
  };

  const sync = async () => {
    setBusy("sync");
    try {
      const next = await syncApplicationCatalogs();
      onSnapshotChanged(next);
      onSuccess(`已更新 ${next.applicationCatalogs.length} 台远端机器的应用目录`);
    } catch (reason) { onError(errorMessage(reason)); }
    finally { setBusy(undefined); }
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!form) return;
    setBusy("save");
    try {
      const existing = snapshot.localApplications.find((application) => application.id === form.id);
      const application: PublishedApplication = {
        id: form.id ?? crypto.randomUUID(),
        name: form.name.trim(),
        launchTarget: form.launchTarget.trim(),
        arguments: form.arguments.trim(),
        workingDirectory: existing?.workingDirectory,
        kind: form.kind,
        source: "manual",
        enabled: existing?.enabled ?? true,
        updatedAtUnixMs: Date.now(),
      };
      onSnapshotChanged(await savePublishedApplication(application));
      setForm(undefined);
      onSuccess(existing ? "应用信息已更新并将在下次连接时发布" : "应用已添加到本机发布目录");
    } catch (reason) { onError(errorMessage(reason)); }
    finally { setBusy(undefined); }
  };

  const toggle = async (application: PublishedApplication) => {
    setBusy(application.id);
    try {
      onSnapshotChanged(await savePublishedApplication({ ...application, enabled: !application.enabled, updatedAtUnixMs: Date.now() }));
    } catch (reason) { onError(errorMessage(reason)); }
    finally { setBusy(undefined); }
  };

  const remove = async (application: PublishedApplication) => {
    setBusy(application.id);
    try {
      onSnapshotChanged(await removePublishedApplication(application.id));
      onSuccess("手工应用已移除");
    } catch (reason) { onError(errorMessage(reason)); }
    finally { setBusy(undefined); }
  };

  const matches = (name: string) => name.toLowerCase().includes(query.trim().toLowerCase());
  const localApplications = snapshot.localApplications.filter((application) => matches(application.name));
  const remoteApplications = snapshot.applicationCatalogs.map((catalog) => ({ ...catalog, applications: catalog.applications.filter((application) => matches(application.name)) }));

  return (
    <div className="view-shell nodes-view">
      <div className="page-heading">
        <div><p className="eyebrow">App Mesh</p><h1>应用节点</h1><p>互联电脑彼此都是节点；应用目录按机器分组，并仅发布安全元数据。</p></div>
        <div className="node-heading-actions">
          <button className="secondary-button" type="button" disabled={Boolean(busy)} onClick={() => void scan()}>{busy === "scan" ? <LoaderCircle className="spin" size={15} /> : <Radar size={15} />}自动扫描</button>
          <button className="primary-button" type="button" onClick={() => setForm({ ...emptyForm })}><Plus size={15} />添加应用</button>
        </div>
      </div>

      <section className="node-command-bar">
        <label><Search size={15} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索所有机器上的应用" /></label>
        <span><Server size={14} />{snapshot.applicationCatalogs.length + 1} 台应用节点</span>
        <button type="button" disabled={Boolean(busy)} onClick={() => void sync()}>{busy === "sync" ? <LoaderCircle className="spin" size={14} /> : <RefreshCw size={14} />}同步远端目录</button>
      </section>

      {form && (
        <form className="application-editor" onSubmit={submit}>
          <div className="editor-heading"><span className="soft-icon"><AppWindow size={17} /></span><div><strong>{form.id ? "维护应用" : "添加本机应用"}</strong><small>启动路径和参数只保存在本机，不会发送给其他设备。</small></div><button type="button" onClick={() => setForm(undefined)}><X size={15} /></button></div>
          <div className="editor-fields">
            <label><span>显示名称</span><input required maxLength={128} value={form.name} onChange={(event) => setForm({ ...form, name: event.target.value })} placeholder="例如 Visual Studio Code" /></label>
            <label className="wide-field"><span>EXE、命令或 .lnk</span><input required value={form.launchTarget} onChange={(event) => setForm({ ...form, launchTarget: event.target.value })} placeholder="C:\\Program Files\\App\\app.exe" /></label>
            <label><span>类型</span><select value={form.kind} onChange={(event) => setForm({ ...form, kind: event.target.value as ApplicationKind })}><option value="gui">GUI 应用</option><option value="terminal">终端工具</option></select></label>
            <label className="wide-field"><span>启动参数（可选）</span><input value={form.arguments} onChange={(event) => setForm({ ...form, arguments: event.target.value })} placeholder="--new-window" /></label>
          </div>
          <div className="editor-actions"><button className="secondary-button" type="button" onClick={() => setForm(undefined)}>取消</button><button className="primary-button" type="submit" disabled={busy === "save"}>{busy === "save" ? <LoaderCircle className="spin" size={14} /> : <Check size={14} />}保存应用</button></div>
        </form>
      )}

      <div className="machine-groups">
        <section className="machine-group local-machine-group">
          <header><span className="machine-avatar"><Laptop size={18} /></span><div><h2>{snapshot.localDevice.name}</h2><p>此电脑 · {snapshot.localApplications.filter((app) => app.enabled).length} 个已发布应用</p></div><em>本机可维护</em></header>
          <div className="application-grid">
            {localApplications.map((application) => (
              <article className={!application.enabled ? "is-disabled" : ""} key={application.id}>
                <span className={`application-icon ${application.kind}`}>{application.kind === "terminal" ? <TerminalSquare size={18} /> : <AppWindow size={18} />}</span>
                <div><h3>{application.name}</h3><p>{application.source === "startMenu" ? "开始菜单扫描" : "手工维护"} · {application.kind === "gui" ? "GUI" : "终端"}</p><small title={application.launchTarget}>{application.launchTarget}</small></div>
                <div className="application-actions"><button type="button" title="编辑" onClick={() => setForm({ id: application.id, name: application.name, launchTarget: application.launchTarget, arguments: application.arguments, kind: application.kind })}><Edit3 size={13} /></button><button className={`mini-toggle ${application.enabled ? "on" : ""}`} type="button" title={application.enabled ? "停止发布" : "启用发布"} disabled={busy === application.id} onClick={() => void toggle(application)}><i /></button>{application.source === "manual" && <button type="button" title="移除" disabled={busy === application.id} onClick={() => void remove(application)}><Trash2 size={13} /></button>}</div>
              </article>
            ))}
            {localApplications.length === 0 && <div className="applications-empty">没有匹配的本机应用。可自动扫描开始菜单，或手工添加 EXE/命令。</div>}
          </div>
        </section>

        {remoteApplications.map((catalog) => (
          <section className="machine-group" key={catalog.deviceId}>
            <header><span className="machine-avatar remote"><Server size={18} /></span><div><h2>{catalog.deviceName}</h2><p>受信远端节点 · {catalog.applications.length} 个可用应用</p></div><em>{new Date(catalog.updatedAtUnixMs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })} 已同步</em></header>
            <div className="application-grid remote-application-grid">
              {catalog.applications.map((application) => <article key={application.id}><span className={`application-icon ${application.kind}`}>{application.kind === "terminal" ? <Command size={18} /> : <AppWindow size={18} />}</span><div><h3>{application.name}</h3><p>{application.kind === "gui" ? "远端 GUI 应用" : "远端终端工具"}</p><small>启动信息由 {catalog.deviceName} 保管</small></div></article>)}
              {catalog.applications.length === 0 && <div className="applications-empty">此节点尚未发布匹配的应用。</div>}
            </div>
          </section>
        ))}

        {snapshot.applicationCatalogs.length === 0 && <section className="remote-node-empty"><Server size={24} /><div><strong>还没有远端应用目录</strong><p>已配对设备在线后点击“同步远端目录”；建立屏幕会话时也会自动双向交换。</p></div></section>}
      </div>
    </div>
  );
}
