!include "LogicLib.nsh"

!macro NSIS_HOOK_POSTINSTALL
  ${IfNot} ${Silent}
    MessageBox MB_ICONINFORMATION|MB_OKCANCEL "PeerSpan 将安装 VirtualDrivers 官方签名的 VDD 虚拟显示驱动，并仅向本地子网开放 PeerSpan 与 Sunshine 入站通信。已有 VDD 配置会被保留。是否继续？" IDOK peerspan_driver_install_confirmed
    Abort
    peerspan_driver_install_confirmed:
  ${EndIf}

  DetailPrint "Installing the signed VirtualDrivers VDD package and local-subnet firewall rules..."
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\driver\install.ps1" -TrustPublisher -AcknowledgeSystemChanges -ApplicationPath "$INSTDIR\${MAINBINARYNAME}.exe" -SunshinePath "$INSTDIR\gamestream\sunshine\Sunshine\sunshine.exe" -InstallerMode'
  Pop $0
  Pop $1
  ${If} $0 != 0
    MessageBox MB_ICONSTOP "VirtualDrivers VDD 或防火墙规则安装失败（退出码 $0）。$\r$\n$1"
    SetErrorLevel $0
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Removing PeerSpan-owned VDD resources and firewall rules..."
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\driver\uninstall.ps1" -RemoveFirewallRules -AcknowledgeSystemChanges -InstallerMode'
  Pop $0
  Pop $1
  ${If} $0 != 0
    MessageBox MB_ICONSTOP "PeerSpan 管理的 VDD 资源或防火墙规则清理失败（退出码 $0）。$\r$\n$1"
    SetErrorLevel $0
    Abort
  ${EndIf}
!macroend
