!include "LogicLib.nsh"

!macro NSIS_HOOK_POSTINSTALL
  ${IfNot} ${Silent}
    MessageBox MB_ICONEXCLAMATION|MB_OKCANCEL "这是 PeerSpan 内部测试安装包。继续将信任 PeerSpan 测试签名证书、安装 IddCx 显示驱动，并仅向本地子网开放 PeerSpan 入站通信。请只在专用测试机上安装。" IDOK peerspan_driver_install_confirmed
    Abort
    peerspan_driver_install_confirmed:
  ${EndIf}

  DetailPrint "Installing the PeerSpan IddCx driver and local-subnet firewall rules..."
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\driver\install-dev.ps1" -Configuration Release -Platform x64 -TrustTestCertificate -AcknowledgeSystemChanges -ApplicationPath "$INSTDIR\${MAINBINARYNAME}.exe" -SunshinePath "$INSTDIR\gamestream\sunshine\Sunshine\sunshine.exe" -InstallerMode'
  Pop $0
  Pop $1
  ${If} $0 != 0
    MessageBox MB_ICONSTOP "PeerSpan 驱动或防火墙规则安装失败（退出码 $0）。$\r$\n$1"
    SetErrorLevel $0
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Removing the PeerSpan IddCx driver, test certificate and firewall rules..."
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\driver\uninstall-dev.ps1" -Configuration Release -Platform x64 -RemoveTestCertificate -RemoveFirewallRules -AcknowledgeSystemChanges -InstallerMode'
  Pop $0
  Pop $1
  ${If} $0 != 0
    MessageBox MB_ICONSTOP "PeerSpan 驱动、测试证书或防火墙规则清理失败（退出码 $0）。$\r$\n$1"
    SetErrorLevel $0
    Abort
  ${EndIf}
!macroend
