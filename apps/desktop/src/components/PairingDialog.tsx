import { Fingerprint, ShieldCheck, X } from "lucide-react";
import { useMemo, useState } from "react";
import type { PeerDevice } from "../types";

interface PairingDialogProps {
  device: PeerDevice;
  busy?: boolean;
  onClose: () => void;
  onConfirm: (code: string) => void | Promise<void>;
}

export function PairingDialog({ device, busy = false, onClose, onConfirm }: PairingDialogProps) {
  const [code, setCode] = useState("");
  const valid = useMemo(() => /^\d{6}$/.test(code), [code]);

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
      <section className="modal-card pairing-dialog" role="dialog" aria-modal="true" aria-labelledby="pairing-title" onMouseDown={(event) => event.stopPropagation()}>
        <button className="modal-close" type="button" aria-label="关闭" onClick={onClose}><X size={18} /></button>
        <span className="modal-symbol"><Fingerprint size={27} /></span>
        <p className="eyebrow">安全配对</p>
        <h2 id="pairing-title">连接到 {device.name}</h2>
        <p className="modal-description">请核对对方屏幕显示的六位数字。只有确认过的设备才能查看画面和发送输入。</p>
        <label className="pair-code-label" htmlFor="pair-code">配对码</label>
        <input
          id="pair-code"
          className="pair-code-input"
          inputMode="numeric"
          autoComplete="one-time-code"
          maxLength={6}
          value={code}
          placeholder="000 000"
          onChange={(event) => setCode(event.target.value.replace(/\D/g, "").slice(0, 6))}
          autoFocus
        />
        <div className="pair-fingerprint">
          <span>设备身份指纹</span>
          <code>{device.fingerprint}</code>
        </div>
        <div className="security-note"><ShieldCheck size={16} /><span>连接将使用设备身份密钥加密，此代码不会写入日志。</span></div>
        <button className="primary-button wide-button" type="button" disabled={!valid || busy} onClick={() => void onConfirm(code)}>{busy ? "正在建立加密通道…" : "核对并配对"}</button>
      </section>
    </div>
  );
}
