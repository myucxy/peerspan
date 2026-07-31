import { KeyRound, ShieldCheck, X } from "lucide-react";
import type { PairingOffer } from "../lib/bridge";
import type { LocalDevice } from "../types";

interface LocalPairingDialogProps {
  device: LocalDevice;
  offer: PairingOffer;
  preview: boolean;
  onClose: () => void;
}

export function LocalPairingDialog({ device, offer, preview, onClose }: LocalPairingDialogProps) {
  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
      <section className="modal-card local-pairing-dialog" role="dialog" aria-modal="true" aria-labelledby="local-pairing-title" onMouseDown={(event) => event.stopPropagation()}>
        <button className="modal-close" type="button" aria-label="关闭本机配对码" onClick={onClose}><X size={18} /></button>
        <span className="modal-symbol"><KeyRound size={26} /></span>
        <p className="eyebrow">允许新设备配对</p>
        <h2 id="local-pairing-title">在另一台电脑输入此代码</h2>
        <p className="modal-description">代码两分钟后失效，最多允许 {offer.attemptsRemaining} 次尝试。配对成功后会自动关闭。</p>
        {preview && <div className="modal-preview-note">设计预览代码，不会开放网络监听</div>}
        <div className="local-pair-code" aria-label={`本机配对码 ${offer.code}`}>
          <span>{offer.code.slice(0, 3)}</span><i /><span>{offer.code.slice(3)}</span>
        </div>
        <div className="pair-fingerprint"><span>{device.name} · 身份指纹</span><code>{device.fingerprint}</code></div>
        <div className="security-note"><ShieldCheck size={16} /><span>代码通过 SPAKE2 派生加密密钥，双方身份还会使用 Ed25519 签名核验。</span></div>
        <button className="secondary-button wide-button" type="button" onClick={onClose}>完成</button>
      </section>
    </div>
  );
}
