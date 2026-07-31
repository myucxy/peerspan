import { beforeEach, describe, expect, it } from "vitest";
import { getAppSnapshot, isDesktopRuntime, savePreferences } from "./bridge";

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
});
