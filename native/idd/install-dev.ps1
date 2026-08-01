[CmdletBinding(SupportsShouldProcess, ConfirmImpact = "High")]
param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",

    [ValidateSet("x64", "ARM64")]
    [string]$Platform = "x64",

    [switch]$TrustTestCertificate,

    [switch]$AcknowledgeSystemChanges,

    [string]$ApplicationPath,

    [switch]$InstallerMode
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not $AcknowledgeSystemChanges) {
    throw "This operation modifies the machine driver store. Re-run with -AcknowledgeSystemChanges after reviewing the script."
}

if ($InstallerMode) {
    $ConfirmPreference = "None"
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "PeerSpan driver installation requires an elevated PowerShell session."
}

function Resolve-NativeSystemTool([string]$Name) {
    $candidates = @(
        (Join-Path $env:WINDIR "Sysnative\$Name"),
        (Join-Path $env:WINDIR "System32\$Name")
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return $candidate
        }
    }
    throw "Required Windows system tool was not found: $Name"
}

$pnpUtil = Resolve-NativeSystemTool "pnputil.exe"

$outputDirectory = Join-Path $PSScriptRoot "$Platform\$Configuration"
$packageDirectory = Join-Path $outputDirectory "PeerSpanIdd"
$inf = Join-Path $packageDirectory "PeerSpanIdd.inf"
$catalog = Join-Path $packageDirectory "peerspanidd.cat"
$driver = Join-Path $packageDirectory "PeerSpanIdd.dll"

foreach ($file in @($inf, $catalog, $driver)) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
        throw "Required driver package file was not found: $file. Run native\idd\build.ps1 first."
    }
}

if ($TrustTestCertificate) {
    $certificate = Join-Path $outputDirectory "PeerSpanIdd.cer"
    if (-not (Test-Path -LiteralPath $certificate -PathType Leaf)) {
        throw "The test certificate was not found at $certificate."
    }
    if ($PSCmdlet.ShouldProcess(
        "LocalMachine Root and TrustedPublisher certificate stores",
        "Trust the PeerSpan development test-signing certificate"
    )) {
        Import-Certificate -FilePath $certificate -CertStoreLocation "Cert:\LocalMachine\Root" | Out-Null
        Import-Certificate -FilePath $certificate -CertStoreLocation "Cert:\LocalMachine\TrustedPublisher" | Out-Null
    }
}

if ($PSCmdlet.ShouldProcess($inf, "Stage and install the PeerSpan IddCx driver package")) {
    & $pnpUtil /add-driver $inf /install
    if ($LASTEXITCODE -ne 0) {
        throw "pnputil failed to install the PeerSpan driver package (exit code $LASTEXITCODE)."
    }
}

if ($ApplicationPath) {
    $resolvedApplicationPath = (Resolve-Path -LiteralPath $ApplicationPath -ErrorAction Stop).Path
    if (-not (Test-Path -LiteralPath $resolvedApplicationPath -PathType Leaf)) {
        throw "The PeerSpan application was not found at $resolvedApplicationPath."
    }

    $firewallRules = @(
        @{
            Name = "PeerSpan-LAN-TCP"
            Protocol = "TCP"
            LocalPort = @("37621", "37622")
        },
        @{
            Name = "PeerSpan-LAN-UDP"
            Protocol = "UDP"
            LocalPort = "Any"
        }
    )

    foreach ($rule in $firewallRules) {
        if ($PSCmdlet.ShouldProcess($rule.Name, "Allow PeerSpan inbound traffic from the local subnet")) {
            Get-NetFirewallRule -Name $rule.Name -ErrorAction SilentlyContinue |
                Remove-NetFirewallRule -ErrorAction Stop
            New-NetFirewallRule `
                -Name $rule.Name `
                -DisplayName $rule.Name `
                -Group "PeerSpan" `
                -Direction Inbound `
                -Action Allow `
                -Enabled True `
                -Profile Domain, Private `
                -Program $resolvedApplicationPath `
                -Protocol $rule.Protocol `
                -LocalPort $rule.LocalPort `
                -RemoteAddress LocalSubnet | Out-Null
        }
    }
}

Write-Output "PeerSpan development driver installation completed. Start the virtual display from the PeerSpan screen page."
