import { Minus, Square, X } from "lucide-react";
import { isDesktopRuntime } from "../lib/bridge";

async function useWindow(action: "minimize" | "toggleMaximize" | "close") {
  if (!isDesktopRuntime()) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow()[action]();
}

export function TitleBar() {
  return (
    <header className="title-bar" data-tauri-drag-region>
      <div className="title-bar-drag" data-tauri-drag-region>
        <span className="title-bar-product">PeerSpan</span>
        <span className="title-bar-separator" />
        <span className="title-bar-context">邻屏控制台</span>
      </div>
      <div className="window-controls" aria-label="窗口控制">
        <button type="button" aria-label="最小化" onClick={() => void useWindow("minimize")}><Minus size={14} /></button>
        <button type="button" aria-label="最大化或还原" onClick={() => void useWindow("toggleMaximize")}><Square size={12} /></button>
        <button type="button" className="window-close" aria-label="关闭" onClick={() => void useWindow("close")}><X size={15} /></button>
      </div>
    </header>
  );
}
