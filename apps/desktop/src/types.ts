export type ViewKey = "home" | "display" | "nodes" | "settings";

export type DeviceStatus = "online" | "busy" | "offline";
export type ScreenEdge = "left" | "right" | "top" | "bottom";
export type QualityMode = "clarity" | "balanced" | "responsive";
export type CapabilityState = "ready" | "requiresSetup" | "planned";

export interface LocalDevice {
  id: string;
  name: string;
  platform: string;
  fingerprint: string;
  publicKey: string;
}

export interface PeerDevice {
  id: string;
  name: string;
  platform: string;
  fingerprint: string;
  publicKey: string;
  status: DeviceStatus;
  trusted: boolean;
  latencyMs?: number;
  lastSeenUnixMs: number;
  addresses: string[];
  controlPort: number;
  protocolVersion: number;
}

export interface Preferences {
  launchAtStartup: boolean;
  autoReconnect: boolean;
  clipboardSync: boolean;
  screenEdge: ScreenEdge;
  quality: QualityMode;
  releaseShortcut: string;
}

export interface Capability {
  state: CapabilityState;
  detail: string;
}

export interface RuntimeCapabilities {
  controlBridge: Capability;
  discovery: Capability;
  securePairing: Capability;
  virtualDisplay: Capability;
  mediaPipeline: Capability;
  inputInjection: Capability;
}

export interface DisplaySession {
  id: string;
  peerId: string;
  direction: "sending" | "receiving";
  state: "negotiating" | "streaming" | "recovering" | "ending";
  widthPx: number;
  heightPx: number;
  refreshHz: number;
  latencyMs?: number;
}

export interface AppSnapshot {
  localDevice: LocalDevice;
  nearbyDevices: PeerDevice[];
  trustedDevices: PeerDevice[];
  activeSession?: DisplaySession;
  preferences: Preferences;
  capabilities: RuntimeCapabilities;
}
