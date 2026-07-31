import { AppWindow, ChevronRight, Command, Cpu, LockKeyhole, Server, TerminalSquare } from "lucide-react";

export function NodesView() {
  return (
    <div className="view-shell nodes-view">
      <div className="page-heading">
        <div><p className="eyebrow">App Node</p><h1>应用节点</h1><p>让无物理显示器的电脑成为可控的远程应用主机。</p></div>
        <span className="milestone-pill">第二个里程碑</span>
      </div>

      <section className="node-hero">
        <div className="node-hero-copy">
          <span className="node-symbol"><Server size={26} /></span>
          <h2>算力在节点，窗口在眼前</h2>
          <p>从一台受信电脑启动节点上的 GUI 应用或命令行工具，文件和运行状态始终留在节点。</p>
          <button className="secondary-button" type="button" disabled>等待扩展屏 MVP 稳定<ChevronRight size={16} /></button>
        </div>
        <div className="node-flow" aria-hidden="true">
          <div className="flow-node"><Server size={22} /><span>Node Service</span><small>Session 0</small></div>
          <div className="flow-arrow"><i /><i /><i /></div>
          <div className="flow-node accented"><AppWindow size={22} /><span>Session Agent</span><small>用户会话</small></div>
          <div className="flow-output"><span><TerminalSquare size={18} /></span><span><AppWindow size={18} /></span></div>
        </div>
      </section>

      <div className="feature-list">
        <article><span className="soft-icon"><Cpu size={19} /></span><div><h3>GUI 应用</h3><p>由用户会话 Agent 启动并追踪窗口组，早期先以整块虚拟桌面呈现。</p></div><span className="feature-status">Node Preview</span></article>
        <article><span className="soft-icon amber"><Command size={19} /></span><div><h3>原生终端</h3><p>使用 ConPTY 传输 UTF-8/VT 数据、窗口尺寸、Ctrl+C 和退出码。</p></div><span className="feature-status">Node Preview</span></article>
        <article><span className="soft-icon slate"><LockKeyhole size={19} /></span><div><h3>权限与审计</h3><p>默认允许列表；Shell、参数、提权和文件传输分别授权。</p></div><span className="feature-status">设计中</span></article>
      </div>
    </div>
  );
}
