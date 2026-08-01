import { AlertCircle, CheckCircle2, LoaderCircle, X } from "lucide-react";
import { useEffect, useState } from "react";
import { PairingDialog } from "./components/PairingDialog";
import { LocalPairingDialog } from "./components/LocalPairingDialog";
import { Sidebar } from "./components/Sidebar";
import { TitleBar } from "./components/TitleBar";
import { useAppSnapshot } from "./hooks/useAppSnapshot";
import { createPairingOffer, endSession, isDesktopRuntime, pairDevice, requestSession, type PairingOffer } from "./lib/bridge";
import type { PeerDevice, ViewKey } from "./types";
import { DisplayView } from "./views/DisplayView";
import { HomeView } from "./views/HomeView";
import { NodesView } from "./views/NodesView";
import { SettingsView } from "./views/SettingsView";

interface ToastState {
  tone: "success" | "error";
  message: string;
}

export default function App() {
  const [activeView, setActiveView] = useState<ViewKey>("home");
  const [pairingDevice, setPairingDevice] = useState<PeerDevice>();
  const [pairingBusy, setPairingBusy] = useState(false);
  const [localPairingOffer, setLocalPairingOffer] = useState<PairingOffer>();
  const [toast, setToast] = useState<ToastState>();
  const { snapshot, loading, scanning, error, scan, updatePreferences } = useAppSnapshot();

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(undefined), 4200);
    return () => window.clearTimeout(timer);
  }, [toast]);

  useEffect(() => {
    if (error) setToast({ tone: "error", message: error });
  }, [error]);

  const selectDevice = async (device: PeerDevice) => {
    if (!device.trusted) {
      setPairingDevice(device);
      return;
    }
    try {
      await requestSession(device.id);
      await scan();
      setActiveView("display");
      setToast({ tone: "success", message: `已与 ${device.name} 建立认证控制会话` });
    } catch (reason) {
      setToast({ tone: "error", message: String(reason) });
      setActiveView("display");
    }
  };

  const stopSession = async (sessionId: string) => {
    try {
      await endSession(sessionId);
      await scan();
      setToast({ tone: "success", message: "屏幕会话已安全结束" });
    } catch (reason) {
      setToast({ tone: "error", message: String(reason) });
    }
  };

  const showLocalPairingOffer = async () => {
    try {
      setLocalPairingOffer(await createPairingOffer());
    } catch (reason) {
      setToast({ tone: "error", message: String(reason) });
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
      setToast({ tone: "error", message: String(reason) });
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
          {activeView === "display" && <DisplayView snapshot={snapshot} onChangePreferences={updatePreferences} onEndSession={stopSession} />}
          {activeView === "nodes" && <NodesView />}
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
