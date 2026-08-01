import { useCallback, useEffect, useState } from "react";
import { getAppSnapshot, isDesktopRuntime, refreshDevices, savePreferences } from "../lib/bridge";
import type { AppSnapshot, Preferences } from "../types";

export function useAppSnapshot() {
  const [snapshot, setSnapshot] = useState<AppSnapshot>();
  const [loading, setLoading] = useState(true);
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string>();

  useEffect(() => {
    let mounted = true;
    getAppSnapshot()
      .then((value) => mounted && setSnapshot(value))
      .catch((reason: unknown) => mounted && setError(String(reason)))
      .finally(() => mounted && setLoading(false));
    return () => { mounted = false; };
  }, []);

  useEffect(() => {
    if (!isDesktopRuntime()) return;
    const interval = window.setInterval(() => {
      refreshDevices().then(setSnapshot).catch((reason: unknown) => setError(String(reason)));
    }, 2_000);
    return () => window.clearInterval(interval);
  }, []);

  const scan = useCallback(async () => {
    setScanning(true);
    setError(undefined);
    try {
      setSnapshot(await refreshDevices());
    } catch (reason) {
      setError(String(reason));
    } finally {
      setScanning(false);
    }
  }, []);

  const updatePreferences = useCallback(async (preferences: Preferences) => {
    setError(undefined);
    const previous = snapshot;
    if (previous) setSnapshot({ ...previous, preferences });
    try {
      setSnapshot(await savePreferences(preferences));
    } catch (reason) {
      if (previous) setSnapshot(previous);
      setError(String(reason));
      throw reason;
    }
  }, [snapshot]);

  return { snapshot, loading, scanning, error, scan, updatePreferences, replaceSnapshot: setSnapshot };
}
