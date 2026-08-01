import { invoke } from "@tauri-apps/api/core";
import type { AppSnapshot, DisplaySession, Preferences, PublishedApplication } from "../types";

export interface PairingOffer {
  code: string;
  expiresAtUnixMs: number;
  attemptsRemaining: number;
}

export const isDesktopRuntime = () => Boolean(window.__TAURI_INTERNALS__);

const previewSnapshot: AppSnapshot = {
  localDevice: {
    id: "9fb367a7-a8b8-4a96-8847-60d8fd89901a",
    name: "此电脑",
    platform: "Windows 11 · 23H2",
    fingerprint: "4D7A 9F20 6C31",
    publicKey: "preview-local-public-key",
  },
  nearbyDevices: [
    {
      id: "f6e156ea-f75d-4c2c-96c8-b582cf840a90",
      name: "书房工作站",
      platform: "Windows 11 · 有线网络",
      fingerprint: "A91C 40D7 2E8B",
      publicKey: "preview-studio-public-key",
      status: "online",
      trusted: true,
      latencyMs: 6,
      lastSeenUnixMs: Date.now(),
      addresses: ["192.168.1.20"],
      controlPort: 37622,
      pairingPort: 37621,
      protocolVersion: 6,
    },
    {
      id: "a43cc1f0-f2f7-48c7-9327-6a278414a3f0",
      name: "Surface Laptop",
      platform: "Windows 10 · Wi-Fi 6",
      fingerprint: "71F8 B03A C265",
      publicKey: "preview-surface-public-key",
      status: "online",
      trusted: false,
      latencyMs: 14,
      lastSeenUnixMs: Date.now(),
      addresses: ["192.168.1.35"],
      controlPort: 37622,
      pairingPort: 37621,
      protocolVersion: 6,
    },
  ],
  trustedDevices: [{
    id: "e10eea54-ea26-4d30-bf85-dff0f20f4379",
    name: "渲染节点",
    platform: "Windows 11 · 有线网络",
    fingerprint: "91A2 B3C4 D5E6",
    publicKey: "preview-render-public-key",
    status: "online",
    trusted: true,
    latencyMs: 9,
    lastSeenUnixMs: Date.now(),
    addresses: ["192.168.1.42"],
    controlPort: 37622,
    pairingPort: 37621,
    protocolVersion: 6,
  }],
  displaySessions: [
    { id: "acbe9277-4ebc-4eaa-a0fb-2631f6057b55", peerId: "f6e156ea-f75d-4c2c-96c8-b582cf840a90", direction: "sending", state: "streaming", widthPx: 1920, heightPx: 1080, refreshHz: 60, latencyMs: 6 },
    { id: "18ed9e27-e93a-47c0-838d-c226891140f3", peerId: "e10eea54-ea26-4d30-bf85-dff0f20f4379", direction: "receiving", state: "recovering", widthPx: 2560, heightPx: 1440, refreshHz: 60, latencyMs: 9 },
  ],
  displayLayouts: [
    { peerId: "f6e156ea-f75d-4c2c-96c8-b582cf840a90", x: 360, y: 28 },
    { peerId: "e10eea54-ea26-4d30-bf85-dff0f20f4379", x: 565, y: 152 },
  ],
  localApplications: [
    { id: "6b6f0a87-2a17-42dc-8bd1-d72f18b999e4", name: "Visual Studio Code", launchTarget: "C:\\Program Files\\Microsoft VS Code\\Code.exe", arguments: "", kind: "gui", source: "manual", enabled: true, updatedAtUnixMs: Date.now() },
  ],
  localCatalogRevision: Date.now(),
  applicationCatalogs: [
    { deviceId: "f6e156ea-f75d-4c2c-96c8-b582cf840a90", deviceName: "书房工作站", revision: 1, updatedAtUnixMs: Date.now(), applications: [{ id: "51856b48-143d-4b1c-bd26-da0c59082b4c", name: "Blender", kind: "gui" }, { id: "67724424-c78d-4f55-91a0-145ae45d3ae5", name: "PowerShell 7", kind: "terminal" }] },
    { deviceId: "e10eea54-ea26-4d30-bf85-dff0f20f4379", deviceName: "渲染节点", revision: 1, updatedAtUnixMs: Date.now(), applications: [{ id: "e52d38bf-630a-44e5-a0f5-467a6727bb28", name: "DaVinci Resolve", kind: "gui" }] },
  ],
  preferences: {
    launchAtStartup: false,
    autoReconnect: true,
    clipboardSync: true,
    screenEdge: "right",
    quality: "balanced",
    releaseShortcut: "Ctrl+Alt+Shift+Esc",
    streamingBackend: "sunshineMoonlight",
  },
  capabilities: {
    controlBridge: { state: "ready", detail: "Web 设计预览" },
    discovery: { state: "planned", detail: "预览模式使用界面样例设备" },
    securePairing: { state: "planned", detail: "Web 预览不启动配对监听器" },
    secureControl: { state: "planned", detail: "Web 预览不启动 TLS 控制监听器" },
    virtualDisplay: { state: "requiresSetup", detail: "虚拟显示驱动尚未安装" },
    streamingBackend: { state: "ready", detail: "Sunshine + Moonlight 高性能核心" },
    mediaPipeline: { state: "ready", detail: "GameStream 硬件编解码与 FEC" },
    inputInjection: { state: "planned", detail: "认证会话建立后启用" },
  },
};

let mutablePreview = structuredClone(previewSnapshot);

export async function getAppSnapshot(): Promise<AppSnapshot> {
  if (isDesktopRuntime()) {
    return invoke<AppSnapshot>("get_app_snapshot");
  }
  await new Promise((resolve) => setTimeout(resolve, 180));
  return structuredClone(mutablePreview);
}

export async function refreshDevices(): Promise<AppSnapshot> {
  if (isDesktopRuntime()) {
    return invoke<AppSnapshot>("refresh_devices");
  }
  await new Promise((resolve) => setTimeout(resolve, 650));
  return structuredClone(mutablePreview);
}

export async function savePreferences(preferences: Preferences): Promise<AppSnapshot> {
  if (isDesktopRuntime()) {
    return invoke<AppSnapshot>("update_preferences", { preferences });
  }
  mutablePreview = { ...mutablePreview, preferences };
  return structuredClone(mutablePreview);
}

export async function requestSession(peerId: string): Promise<DisplaySession> {
  if (isDesktopRuntime()) {
    return invoke<DisplaySession>("request_display_session", { peerId });
  }
  await new Promise((resolve) => setTimeout(resolve, 600));
  throw new Error("设计预览不会建立真实串流会话");
}

export async function endSession(sessionId: string): Promise<void> {
  if (isDesktopRuntime()) {
    await invoke("end_display_session", { sessionId });
    return;
  }
  throw new Error("设计预览没有可结束的真实会话");
}

export async function setDisplayLayout(peerId: string, x: number, y: number): Promise<AppSnapshot> {
  if (isDesktopRuntime()) return invoke<AppSnapshot>("set_display_layout", { peerId, x, y });
  const existing = mutablePreview.displayLayouts.find((layout) => layout.peerId === peerId);
  if (existing) Object.assign(existing, { x, y });
  else mutablePreview.displayLayouts.push({ peerId, x, y });
  return structuredClone(mutablePreview);
}

export async function scanPublishedApplications(): Promise<AppSnapshot> {
  if (isDesktopRuntime()) return invoke<AppSnapshot>("scan_published_applications");
  return structuredClone(mutablePreview);
}

export async function savePublishedApplication(application: PublishedApplication): Promise<AppSnapshot> {
  if (isDesktopRuntime()) return invoke<AppSnapshot>("save_published_application", { application });
  const index = mutablePreview.localApplications.findIndex((item) => item.id === application.id);
  if (index >= 0) mutablePreview.localApplications[index] = application;
  else mutablePreview.localApplications.push(application);
  return structuredClone(mutablePreview);
}

export async function removePublishedApplication(applicationId: string): Promise<AppSnapshot> {
  if (isDesktopRuntime()) return invoke<AppSnapshot>("remove_published_application", { applicationId });
  mutablePreview.localApplications = mutablePreview.localApplications.filter((item) => item.id !== applicationId);
  return structuredClone(mutablePreview);
}

export async function syncApplicationCatalogs(): Promise<AppSnapshot> {
  if (isDesktopRuntime()) return invoke<AppSnapshot>("sync_application_catalogs");
  return structuredClone(mutablePreview);
}

export async function startVirtualDisplay(): Promise<AppSnapshot> {
  if (isDesktopRuntime()) {
    return invoke<AppSnapshot>("start_virtual_display");
  }
  throw new Error("设计预览不能创建 Windows 虚拟显示器");
}

export async function stopVirtualDisplay(): Promise<AppSnapshot> {
  if (isDesktopRuntime()) {
    return invoke<AppSnapshot>("stop_virtual_display");
  }
  throw new Error("设计预览没有可撤销的 Windows 虚拟显示器");
}

export async function createPairingOffer(): Promise<PairingOffer> {
  if (isDesktopRuntime()) {
    return invoke<PairingOffer>("create_pairing_offer");
  }
  return {
    code: "482913",
    expiresAtUnixMs: Date.now() + 120_000,
    attemptsRemaining: 5,
  };
}

export async function pairDevice(peerId: string, code: string): Promise<void> {
  if (isDesktopRuntime()) {
    await invoke("pair_device", { peerId, code });
    return;
  }
  await new Promise((resolve) => setTimeout(resolve, 550));
  throw new Error("设计预览不会发送配对码或保存信任关系");
}
