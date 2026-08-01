import { beforeEach, describe, expect, it } from "vitest";
import { getAppSnapshot, isDesktopRuntime, savePreferences, startVirtualDisplay, stopVirtualDisplay } from "./bridge";

describe("web design-preview bridge", () => {
  beforeEach(() => {
    delete window.__TAURI_INTERNALS__;
  });

  it("is explicitly outside the desktop runtime", () => {
    expect(isDesktopRuntime()).toBe(false);
  });

  it("returns isolated snapshots and preserves preference changes", async () => {
    const initial = await getAppSnapshot();
    const updated = await savePreferences({ ...initial.preferences, screenEdge: "left" });
    initial.preferences.screenEdge = "bottom";

    expect(updated.preferences.screenEdge).toBe("left");
    expect((await getAppSnapshot()).preferences.screenEdge).toBe("left");
  });

  it("refuses native virtual-display mutations in the design preview", async () => {
    await expect(startVirtualDisplay()).rejects.toThrow("设计预览不能创建 Windows 虚拟显示器");
    await expect(stopVirtualDisplay()).rejects.toThrow("设计预览没有可撤销的 Windows 虚拟显示器");
  });
});
