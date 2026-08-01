import { AlertCircle, CheckCircle2, LoaderCircle, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { PairingDialog } from "./components/PairingDialog";
import { LocalPairingDialog } from "./components/LocalPairingDialog";
import { Sidebar } from "./components/Sidebar";
import { TitleBar } from "./components/TitleBar";
import { useAppSnapshot } from "./hooks/useAppSnapshot";
import { createPairingOffer, endSession, isDesktopRuntime, pairDevice, requestSession, setDisplayLayout, startVirtualDisplay, stopVirtualDisplay, type PairingOffer } from "./lib/bridge";
import type { DisplaySession, PeerDevice, ViewKey } from "./types";
import { DisplayView } from "./views/DisplayView";
import { HomeView } from "./views/HomeView";
import { NodesView } from "./views/NodesView";
import { SettingsView } from "./views/SettingsView";

interface ToastState {
  tone: "success" | "error";
  message: string;
}

const errorMessage = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);

export default function App() {
  const [activeView, setActiveView] = useState<ViewKey>("home");
  const [pairingDevice, setPairingDevice] = useState<PeerDevice>();
  const [pairingBusy, setPairingBusy] = useState(false);
  const [localPairingOffer, setLocalPairingOffer] = useState<PairingOffer>();
  const [toast, setToast] = useState<ToastState>();
  const [virtualDisplayBusy, setVirtualDisplayBusy] = useState(false);
  const previousSessions = useRef(new Map<string, DisplaySession>());
  const reconnectPeers = useRef(new Set<string>());
  const reconnectBusy = useRef(new Set<string>());
  const intentionallyEnded = useRef(new Set<string>());
  const { snapshot, loading, scanning, error, scan, updatePreferences, replaceSnapshot } = useAppSnapshot();

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(undefined), 4200);
    return () => window.clearTimeout(timer);
  }, [toast]);

  useEffect(() => {
    if (error) setToast({ tone: "error", message: error });
  }, [error]);

  useEffect(() => {
    if (!snapshot || !isDesktopRuntime()) return;
    const current = new Map(snapshot.displaySessions.map((session) => [session.id, session]));
    for (const previous of previousSessions.current.values()) {
      if (current.has(previous.id)) continue;
      if (intentionallyEnded.current.delete(previous.id)) continue;
      if (previous.direction === "sending" && snapshot.preferences.autoReconnect) {
        reconnectPeers.current.add(previous.peerId);
        setToast({ tone: "error", message: "一台设备的连接已中断，其他会话不受影响；PeerSpan 将独立重连" });
      }
    }
    for (const session of current.values()) reconnectPeers.current.delete(session.peerId);
    previousSessions.current = current;
    if (!snapshot.preferences.autoReconnect) {
      reconnectPeers.current.clear();
      return;
    }
    for (const peerId of reconnectPeers.current) {
      if (reconnectBusy.current.has(peerId)) continue;
      if (snapshot.displaySessions.some((session) => session.peerId === peerId)) continue;
      const peer = snapshot.nearbyDevices.find((device) => device.id === peerId && device.status === "online");
      if (!peer) continue;
      reconnectBusy.current.add(peerId);
      requestSession(peerId)
        .then(async () => {
          reconnectPeers.current.delete(peerId);
          await scan();
          setToast({ tone: "success", message: `已自动恢复与 ${peer.name} 的屏幕会话` });
        })
        .catch(() => {
          // The 2-second device refresh drives the next bounded reconnect attempt.
        })
        .finally(() => reconnectBusy.current.delete(peerId));
    }
  }, [snapshot, scan]);

  const selectDevice = async (device: PeerDevice) => {
    if (!device.trusted) {
      setPairingDevice(device);
      return;
    }
    if (snapshot?.displaySessions.some((session) => session.peerId === device.id)) {
      setActiveView("display");
      setToast({ tone: "success", message: `${device.name} 的屏幕会话已在运行` });
      return;
    }
    try {
      await requestSession(device.id);
      await scan();
      setActiveView("display");
      setToast({ tone: "success", message: `已与 ${device.name} 建立认证控制会话` });
    } catch (reason) {
      setToast({ tone: "error", message: errorMessage(reason) });
      setActiveView("display");
    }
  };

  const updateDisplayLayout = async (peerId: string, x: number, y: number) => {
    try {
      replaceSnapshot(await setDisplayLayout(peerId, x, y));
    } catch (reason) {
      setToast({ tone: "error", message: errorMessage(reason) });
    }
  };

  const stopSession = async (sessionId: string) => {
    intentionallyEnded.current.add(sessionId);
    try {
      await endSession(sessionId);
      await scan();
      setToast({ tone: "success", message: "屏幕会话已安全结束" });
    } catch (reason) {
      intentionallyEnded.current.delete(sessionId);
      setToast({ tone: "error", message: errorMessage(reason) });
    }
  };

  const setVirtualDisplay = async (enabled: boolean) => {
    setVirtualDisplayBusy(true);
    try {
      if (enabled) await startVirtualDisplay();
      else await stopVirtualDisplay();
      await scan();
      setToast({ tone: "success", message: enabled ? "PeerSpan 虚拟显示器已启用" : "PeerSpan 虚拟显示器已安全撤销" });
    } catch (reason) {
      await scan();
      setToast({ tone: "error", message: errorMessage(reason) });
    } finally {
      setVirtualDisplayBusy(false);
    }
  };

  const showLocalPairingOffer = async () => {
    try {
      setLocalPairingOffer(await createPairingOffer());
    } catch (reason) {
      setToast({ tone: "error", message: errorMessage(reason) });
    }
  };

  const confirmPairing = async (code: string) => {
    if (!pairingDevice) return;
    setPairingBusy(true);
    try {
      await pairDevice(pairingDevice.id, code);
      setPairingDevice(undefined);
      await scan();
      setToast({ tone: "success", message: `已安全配对 ${pairingDevice.name}` });
    } catch (reason) {
      setToast({ tone: "error", message: errorMessage(reason) });
    } finally {
      setPairingBusy(false);
    }
  };

  if (loading || !snapshot) {
    return (
      <main className="loading-screen">
        <span className="loading-mark"><i /><i /></span>
        <LoaderCircle className="spin" size={21} />
        <strong>正在准备 PeerSpan</strong>
        <p>读取设备身份与运行能力…</p>
      </main>
    );
  }

  return (
    <div className="app-frame">
      <TitleBar />
      <div className="app-body">
        <Sidebar active={activeView} localDevice={snapshot.localDevice} onNavigate={setActiveView} />
        <main className="content-area">
          {activeView === "home" && <HomeView snapshot={snapshot} scanning={scanning} preview={!isDesktopRuntime()} onScan={() => void scan()} onSelectDevice={(device) => void selectDevice(device)} onOpenDisplay={() => setActiveView("display")} onCreatePairingOffer={() => void showLocalPairingOffer()} />}
          {activeView === "display" && <DisplayView snapshot={snapshot} virtualDisplayBusy={virtualDisplayBusy} onChangePreferences={updatePreferences} onSetVirtualDisplay={setVirtualDisplay} onSetLayout={updateDisplayLayout} onEndSession={stopSession} />}
          {activeView === "nodes" && <NodesView snapshot={snapshot} onSnapshotChanged={replaceSnapshot} onError={(message) => setToast({ tone: "error", message })} onSuccess={(message) => setToast({ tone: "success", message })} />}
          {activeView === "settings" && <SettingsView snapshot={snapshot} onChangePreferences={updatePreferences} />}
        </main>
      </div>

      {pairingDevice && (
        <PairingDialog
          device={pairingDevice}
          busy={pairingBusy}
          onClose={() => setPairingDevice(undefined)}
          onConfirm={confirmPairing}
        />
      )}

      {localPairingOffer && (
        <LocalPairingDialog
          device={snapshot.localDevice}
          offer={localPairingOffer}
          preview={!isDesktopRuntime()}
          onClose={() => setLocalPairingOffer(undefined)}
        />
      )}

      {toast && (
        <div className={`toast toast-${toast.tone}`} role="status">
          {toast.tone === "success" ? <CheckCircle2 size={18} /> : <AlertCircle size={18} />}
          <span>{toast.message}</span>
          <button type="button" aria-label="关闭提示" onClick={() => setToast(undefined)}><X size={15} /></button>
        </div>
      )}
    </div>
  );
}
