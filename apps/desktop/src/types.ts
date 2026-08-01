export type ViewKey = "home" | "display" | "nodes" | "settings";

export type DeviceStatus = "online" | "busy" | "offline";
export type ScreenEdge = "left" | "right" | "top" | "bottom";
export type QualityMode = "clarity" | "balanced" | "responsive";
export type StreamingBackend = "sunshineMoonlight" | "native";
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
  pairingPort: number;
  protocolVersion: number;
}

export interface Preferences {
  launchAtStartup: boolean;
  autoReconnect: boolean;
  clipboardSync: boolean;
  screenEdge: ScreenEdge;
  quality: QualityMode;
  releaseShortcut: string;
  streamingBackend: StreamingBackend;
}

export interface Capability {
  state: CapabilityState;
  detail: string;
}

export interface RuntimeCapabilities {
  controlBridge: Capability;
  discovery: Capability;
  securePairing: Capability;
  secureControl: Capability;
  virtualDisplay: Capability;
  streamingBackend: Capability;
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

export interface DisplayLayout {
  peerId: string;
  x: number;
  y: number;
}

export type ApplicationKind = "gui" | "terminal";
export type ApplicationSource = "manual" | "startMenu";

export interface PublishedApplication {
  id: string;
  name: string;
  launchTarget: string;
  arguments: string;
  workingDirectory?: string;
  kind: ApplicationKind;
  source: ApplicationSource;
  enabled: boolean;
  updatedAtUnixMs: number;
}

export interface ApplicationSummary {
  id: string;
  name: string;
  kind: ApplicationKind;
}

export interface ApplicationCatalog {
  deviceId: string;
  deviceName: string;
  revision: number;
  updatedAtUnixMs: number;
  applications: ApplicationSummary[];
}

export interface AppSnapshot {
  localDevice: LocalDevice;
  nearbyDevices: PeerDevice[];
  trustedDevices: PeerDevice[];
  displaySessions: DisplaySession[];
  displayLayouts: DisplayLayout[];
  localApplications: PublishedApplication[];
  localCatalogRevision: number;
  applicationCatalogs: ApplicationCatalog[];
  preferences: Preferences;
  capabilities: RuntimeCapabilities;
}
