import { invoke } from "@tauri-apps/api/core";
import type { AppSnapshot, DisplaySession, Preferences } from "../types";

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
      protocolVersion: 2,
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
      protocolVersion: 2,
    },
  ],
  trustedDevices: [],
  preferences: {
    launchAtStartup: false,
    autoReconnect: true,
    clipboardSync: true,
    screenEdge: "right",
    quality: "balanced",
    releaseShortcut: "Ctrl+Alt+Shift+Esc",
  },
  capabilities: {
    controlBridge: { state: "ready", detail: "Web 设计预览" },
    discovery: { state: "planned", detail: "预览模式使用界面样例设备" },
    securePairing: { state: "planned", detail: "Web 预览不启动配对监听器" },
    secureControl: { state: "planned", detail: "Web 预览不启动 TLS 控制监听器" },
    virtualDisplay: { state: "requiresSetup", detail: "虚拟显示驱动尚未安装" },
    mediaPipeline: { state: "planned", detail: "D3D11 媒体管线技术尖刺待完成" },
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
