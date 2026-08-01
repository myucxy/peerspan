[CmdletBinding(SupportsShouldProcess, ConfirmImpact = "High")]
param(
    [switch]$AcknowledgeSystemChanges,

    [switch]$RemoveFirewallRules,

    [switch]$InstallerMode
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not $AcknowledgeSystemChanges) {
    throw "This operation removes display devices and driver packages. Re-run with -AcknowledgeSystemChanges after reviewing the script."
}

if ($InstallerMode) {
    $ConfirmPreference = "None"
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "VirtualDrivers VDD removal requires an elevated PowerShell session."
}

function Resolve-NativeSystemTool([string]$Name) {
    foreach ($candidate in @(
        (Join-Path $env:WINDIR "Sysnative\$Name"),
        (Join-Path $env:WINDIR "System32\$Name")
    )) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return $candidate
        }
    }
    throw "Required Windows system tool was not found: $Name"
}

function Find-DriverPackages([string]$PnpUtil, [string]$OriginalInf) {
    $output = @(& $PnpUtil /enum-drivers)
    if ($LASTEXITCODE -ne 0) {
        throw "pnputil failed to enumerate driver packages (exit code $LASTEXITCODE)."
    }
    $pattern = "(?i)\b$([Regex]::Escape($OriginalInf))\b"
    @(($output -join "`n") -split "(?:\r?\n){2,}" | ForEach-Object {
        if ($_ -match $pattern -and $_ -match '(?i)\b(oem\d+\.inf)\b') {
            $Matches[1].ToLowerInvariant()
        }
    } | Sort-Object -Unique)
}

function Remove-CertificateFromStore([string]$Thumbprint, [string]$StoreName) {
    $store = [Security.Cryptography.X509Certificates.X509Store]::new(
        $StoreName,
        [Security.Cryptography.X509Certificates.StoreLocation]::LocalMachine
    )
    try {
        $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
        $matches = $store.Certificates.Find(
            [Security.Cryptography.X509Certificates.X509FindType]::FindByThumbprint,
            $Thumbprint,
            $false
        )
        foreach ($match in $matches) {
            $store.Remove($match)
        }
    } finally {
        $store.Close()
    }
}

function Get-StateValue($State, [string]$Name, $DefaultValue) {
    if ($null -ne $State -and $State.PSObject.Properties.Name -contains $Name) {
        return $State.$Name
    }
    return $DefaultValue
}

$pnpUtil = Resolve-NativeSystemTool "pnputil.exe"
$stateRoot = Join-Path $env:ProgramData "PeerSpan"
$statePath = Join-Path $stateRoot "vdd-install-state.json"
$state = $null
if (Test-Path -LiteralPath $statePath -PathType Leaf) {
    $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
}

foreach ($instanceId in @(
    "SWD\MttVDD\PeerSpanVirtualDisplay",
    "SWD\PeerSpanVirtualDisplay\PeerSpanVirtualDisplay"
)) {
    $deviceOutput = @(& $pnpUtil /enum-devices /instanceid $instanceId)
    if ($LASTEXITCODE -eq 0 -and ($deviceOutput -join "`n") -match [Regex]::Escape($instanceId)) {
        if ($PSCmdlet.ShouldProcess($instanceId, "Remove the PeerSpan virtual display device node")) {
            & $pnpUtil /remove-device $instanceId
            if ($LASTEXITCODE -ne 0) {
                throw "pnputil failed to remove $instanceId (exit code $LASTEXITCODE)."
            }
        }
    }
}

$legacyPackages = @(Find-DriverPackages -PnpUtil $pnpUtil -OriginalInf "PeerSpanIdd.inf")
foreach ($packageName in $legacyPackages) {
    if ($PSCmdlet.ShouldProcess($packageName, "Remove the legacy PeerSpan IddCx prototype package")) {
        & $pnpUtil /delete-driver $packageName /uninstall /force
        if ($LASTEXITCODE -ne 0) {
            throw "pnputil failed to remove legacy package $packageName (exit code $LASTEXITCODE)."
        }
    }
}

if ([bool](Get-StateValue $state "driverPackageOwned" $false)) {
    $ownedPackages = @(Get-StateValue $state "driverPackages" @())
    if (-not $ownedPackages) {
        $ownedPackages = @(Find-DriverPackages -PnpUtil $pnpUtil -OriginalInf "MttVDD.inf")
    }
    foreach ($packageName in $ownedPackages) {
        if ($packageName -notmatch '(?i)^oem\d+\.inf$') {
            throw "Refusing to remove an invalid recorded driver package name: $packageName"
        }
        if ($PSCmdlet.ShouldProcess($packageName, "Remove the VDD package installed by PeerSpan")) {
            & $pnpUtil /delete-driver $packageName /uninstall /force
            if ($LASTEXITCODE -ne 0) {
                throw "pnputil failed to remove $packageName (exit code $LASTEXITCODE)."
            }
        }
    }
}

if ([bool](Get-StateValue $state "publisherCertificateOwned" $false)) {
    $thumbprint = [string](Get-StateValue $state "publisherThumbprint" "")
    if ($thumbprint -notmatch '^[0-9A-Fa-f]{40}$') {
        throw "Refusing to remove an invalid recorded publisher thumbprint."
    }
    if ($PSCmdlet.ShouldProcess(
        "LocalMachine TrustedPublisher certificate store",
        "Remove the VDD publisher certificate added by PeerSpan"
    )) {
        Remove-CertificateFromStore -Thumbprint $thumbprint -StoreName "TrustedPublisher"
    }
}

if ([bool](Get-StateValue $state "configurationOwned" $false)) {
    $configurationPath = [string](Get-StateValue $state "configurationPath" "")
    $expectedConfigurationPath = Join-Path $env:SystemDrive "VirtualDisplayDriver\vdd_settings.xml"
    if (-not $configurationPath.Equals($expectedConfigurationPath, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to remove an unexpected VDD configuration path: $configurationPath"
    }
    if ((Test-Path -LiteralPath $configurationPath -PathType Leaf) -and
        $PSCmdlet.ShouldProcess($configurationPath, "Remove the VDD configuration created by PeerSpan")) {
        Remove-Item -LiteralPath $configurationPath -Force
        $configurationRoot = Split-Path -Parent $configurationPath
        if (-not (Get-ChildItem -LiteralPath $configurationRoot -Force)) {
            Remove-Item -LiteralPath $configurationRoot -Force
        }
    }
}

if ($RemoveFirewallRules) {
    $netsh = Resolve-NativeSystemTool "netsh.exe"
    foreach ($ruleName in @(
        "PeerSpan-LAN-TCP",
        "PeerSpan-LAN-UDP",
        "PeerSpan-Sunshine-LAN-TCP",
        "PeerSpan-Sunshine-LAN-UDP"
    )) {
        if ($PSCmdlet.ShouldProcess($ruleName, "Remove the PeerSpan local-subnet firewall rule")) {
            & $netsh advfirewall firewall delete rule "name=$ruleName" | Out-Null
            if ($LASTEXITCODE -ne 0) {
                throw "netsh failed to remove firewall rule $ruleName (exit code $LASTEXITCODE)."
            }
        }
    }
}

if ((Test-Path -LiteralPath $statePath -PathType Leaf) -and
    $PSCmdlet.ShouldProcess($statePath, "Remove the PeerSpan VDD ownership record")) {
    Remove-Item -LiteralPath $statePath -Force
}

Write-Output "PeerSpan VDD devices and PeerSpan-owned driver resources were removed. Pre-existing VDD installations were preserved."
