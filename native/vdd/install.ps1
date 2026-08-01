[CmdletBinding(SupportsShouldProcess, ConfirmImpact = "High")]
param(
    [switch]$AcknowledgeSystemChanges,

    [string]$PackageDirectory,

    [string]$ApplicationPath,

    [string]$SunshinePath,

    [switch]$TrustPublisher,

    [switch]$InstallerMode
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not $AcknowledgeSystemChanges) {
    throw "This operation installs a display driver and modifies machine configuration. Re-run with -AcknowledgeSystemChanges after reviewing the script."
}

if ($InstallerMode) {
    $ConfirmPreference = "None"
}

if ([string]::IsNullOrWhiteSpace($PackageDirectory)) {
    $PackageDirectory = Join-Path $PSScriptRoot "package"
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "VirtualDrivers VDD installation requires an elevated PowerShell session."
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

function Test-CertificateInStore(
    [Security.Cryptography.X509Certificates.X509Certificate2]$Certificate,
    [string]$StoreName
) {
    $store = [Security.Cryptography.X509Certificates.X509Store]::new(
        $StoreName,
        [Security.Cryptography.X509Certificates.StoreLocation]::LocalMachine
    )
    try {
        $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadOnly)
        $matches = $store.Certificates.Find(
            [Security.Cryptography.X509Certificates.X509FindType]::FindByThumbprint,
            $Certificate.Thumbprint,
            $false
        )
        return $matches.Count -gt 0
    } finally {
        $store.Close()
    }
}

function Add-CertificateToStore(
    [Security.Cryptography.X509Certificates.X509Certificate2]$Certificate,
    [string]$StoreName
) {
    $store = [Security.Cryptography.X509Certificates.X509Store]::new(
        $StoreName,
        [Security.Cryptography.X509Certificates.StoreLocation]::LocalMachine
    )
    try {
        $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
        $store.Add($Certificate)
    } finally {
        $store.Close()
    }
}

function Test-CertificateThumbprintInStore([string]$Thumbprint, [string]$StoreName) {
    $store = [Security.Cryptography.X509Certificates.X509Store]::new(
        $StoreName,
        [Security.Cryptography.X509Certificates.StoreLocation]::LocalMachine
    )
    try {
        $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadOnly)
        return $store.Certificates.Find(
            [Security.Cryptography.X509Certificates.X509FindType]::FindByThumbprint,
            $Thumbprint,
            $false
        ).Count -gt 0
    } finally {
        $store.Close()
    }
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

function Get-Sha256([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try {
        $bytes = $algorithm.ComputeHash($stream)
        return -join @($bytes | ForEach-Object { $_.ToString("X2") })
    } finally {
        $algorithm.Dispose()
        $stream.Dispose()
    }
}

$packageDirectory = (Resolve-Path -LiteralPath $PackageDirectory -ErrorAction Stop).Path
$expectedHashes = [ordered]@{
    "mttvdd.cat" = "08A0093FC9B2E32B287A6F8A77CA4DE0A31830D29FC33D2B13A918DC859468F6"
    "MttVDD.dll" = "C9CA837F57A98FBD43BC416A7F535A95843626E7759EAF85CF0CD7CE334DBB05"
    "MttVDD.inf" = "550D211FE481E74DFE3F9D724ED78BE48B3A9113405965D683D9373E8D672F5D"
    "vdd_settings.xml" = "EDB2501D6D5DA17F66D15D4B97A6F4A3F0D8963165AC4A6A6259D95118288020"
}
foreach ($entry in $expectedHashes.GetEnumerator()) {
    $path = Join-Path $packageDirectory $entry.Key
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Required VDD package file was not found: $path"
    }
    $actualHash = Get-Sha256 -Path $path
    if ($actualHash -ne $entry.Value) {
        throw "VDD package hash mismatch for $($entry.Key). Expected $($entry.Value), received $actualHash."
    }
}

$catalog = Join-Path $packageDirectory "mttvdd.cat"
$catalogCertificates = [Security.Cryptography.X509Certificates.X509Certificate2Collection]::new()
$catalogCertificates.Import([IO.File]::ReadAllBytes($catalog))
$publisherCertificate = @($catalogCertificates | Where-Object {
    $_.Thumbprint -eq "3CF8CF26D8BA266C3A483AB7D26D4A818E317D76"
}) | Select-Object -First 1
if ($null -eq $publisherCertificate) {
    throw "The pinned SignPath Foundation certificate was not found in the VDD catalog."
}

$stateRoot = Join-Path $env:ProgramData "PeerSpan"
$statePath = Join-Path $stateRoot "vdd-install-state.json"
$previousState = $null
if (Test-Path -LiteralPath $statePath -PathType Leaf) {
    $previousState = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
}
$pnpUtil = Resolve-NativeSystemTool "pnputil.exe"
$packagesBefore = @(Find-DriverPackages -PnpUtil $pnpUtil -OriginalInf "MttVDD.inf")
$driverPackageOwned = [bool](Get-StateValue $previousState "driverPackageOwned" $false)
$publisherCertificateOwned = [bool](Get-StateValue $previousState "publisherCertificateOwned" $false)
$configurationOwned = [bool](Get-StateValue $previousState "configurationOwned" $false)

if ($TrustPublisher) {
    $publisherWasTrusted = Test-CertificateInStore `
        -Certificate $publisherCertificate `
        -StoreName "TrustedPublisher"
    if (-not $publisherWasTrusted -and $PSCmdlet.ShouldProcess(
        "LocalMachine TrustedPublisher certificate store",
        "Trust the pinned SignPath Foundation certificate used by the official VDD catalog"
    )) {
        Add-CertificateToStore `
            -Certificate $publisherCertificate `
            -StoreName "TrustedPublisher"
        $publisherCertificateOwned = $true
    }
}

$inf = Join-Path $packageDirectory "MttVDD.inf"
if ($PSCmdlet.ShouldProcess($inf, "Stage the officially signed VirtualDrivers VDD package")) {
    & $pnpUtil /add-driver $inf /install
    $pnpExitCode = $LASTEXITCODE
    # Windows 10 pnputil returns ERROR_NO_MORE_ITEMS (259) when the identical
    # package is already the best driver for an existing phantom software device.
    if ($pnpExitCode -notin @(0, 259)) {
        throw "pnputil failed to stage the VirtualDrivers VDD package (exit code $LASTEXITCODE)."
    }
}

$packagesAfter = @(Find-DriverPackages -PnpUtil $pnpUtil -OriginalInf "MttVDD.inf")
if (-not $packagesAfter) {
    throw "The VirtualDrivers VDD package was not found in the driver store after pnputil completed."
}
if (-not $driverPackageOwned -and -not $packagesBefore) {
    $driverPackageOwned = $true
}

# An in-place upgrade no longer uses the test-signed PeerSpan sample driver. Remove
# only that PeerSpan-specific device/package after the signed VDD is safely staged.
$legacyInstanceId = "SWD\PeerSpanVirtualDisplay\PeerSpanVirtualDisplay"
$legacyDeviceOutput = @(& $pnpUtil /enum-devices /instanceid $legacyInstanceId)
if ($LASTEXITCODE -eq 0 -and
    ($legacyDeviceOutput -join "`n") -match [Regex]::Escape($legacyInstanceId) -and
    $PSCmdlet.ShouldProcess($legacyInstanceId, "Remove the legacy PeerSpan virtual display device")) {
    & $pnpUtil /remove-device $legacyInstanceId
    if ($LASTEXITCODE -ne 0) {
        throw "pnputil failed to remove the legacy PeerSpan device (exit code $LASTEXITCODE)."
    }
}
$legacyPackages = @(Find-DriverPackages -PnpUtil $pnpUtil -OriginalInf "PeerSpanIdd.inf")
foreach ($legacyPackage in $legacyPackages) {
    if ($PSCmdlet.ShouldProcess($legacyPackage, "Remove the legacy PeerSpan IddCx prototype package")) {
        & $pnpUtil /delete-driver $legacyPackage /uninstall /force
        if ($LASTEXITCODE -ne 0) {
            throw "pnputil failed to remove legacy package $legacyPackage (exit code $LASTEXITCODE)."
        }
    }
}
$legacyTestCertificateThumbprint = "9AADD10ED9B76B3934414EA36E4B1FEDCF701706"
foreach ($storeName in @("Root", "TrustedPublisher")) {
    if ((Test-CertificateThumbprintInStore `
        -Thumbprint $legacyTestCertificateThumbprint `
        -StoreName $storeName) -and
        $PSCmdlet.ShouldProcess(
            "LocalMachine $storeName certificate store",
            "Remove the known legacy PeerSpan WDK test certificate"
        )) {
        Remove-CertificateFromStore `
            -Thumbprint $legacyTestCertificateThumbprint `
            -StoreName $storeName
    }
}

$configurationRoot = Join-Path $env:SystemDrive "VirtualDisplayDriver"
$configurationPath = Join-Path $configurationRoot "vdd_settings.xml"
if (-not (Test-Path -LiteralPath $configurationPath -PathType Leaf)) {
    if ($PSCmdlet.ShouldProcess($configurationPath, "Install the one-monitor VDD configuration")) {
        New-Item -ItemType Directory -Path $configurationRoot -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $packageDirectory "vdd_settings.xml") `
            -Destination $configurationPath -Force
        $configurationOwned = $true
    }
}

New-Item -ItemType Directory -Path $stateRoot -Force | Out-Null
$state = [ordered]@{
    schemaVersion = 1
    release = "25.7.23"
    driverVersion = "11.30.4.434"
    driverPackageOwned = $driverPackageOwned
    driverPackages = @($packagesAfter)
    publisherCertificateOwned = $publisherCertificateOwned
    publisherThumbprint = $publisherCertificate.Thumbprint
    configurationOwned = $configurationOwned
    configurationPath = $configurationPath
    updatedAtUtc = [DateTime]::UtcNow.ToString("o")
}
$state | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $statePath -Encoding UTF8

if ($ApplicationPath) {
    $resolvedApplicationPath = (Resolve-Path -LiteralPath $ApplicationPath -ErrorAction Stop).Path
    $netsh = Resolve-NativeSystemTool "netsh.exe"
    $firewallRules = @(
        @{ Name = "PeerSpan-LAN-TCP"; Protocol = "TCP"; LocalPort = "37621,37622" },
        @{ Name = "PeerSpan-LAN-UDP"; Protocol = "UDP"; LocalPort = $null }
    )
    foreach ($rule in $firewallRules) {
        if ($PSCmdlet.ShouldProcess($rule.Name, "Allow PeerSpan inbound traffic from the local subnet")) {
            & $netsh advfirewall firewall delete rule "name=$($rule.Name)" | Out-Null
            $arguments = @(
                "advfirewall", "firewall", "add", "rule",
                "name=$($rule.Name)", "dir=in", "action=allow", "enable=yes",
                "profile=domain,private", "program=$resolvedApplicationPath",
                "protocol=$($rule.Protocol)", "remoteip=LocalSubnet"
            )
            if ($rule.LocalPort) {
                $arguments += "localport=$($rule.LocalPort)"
            }
            & $netsh @arguments | Out-Null
            if ($LASTEXITCODE -ne 0) {
                throw "netsh failed to create firewall rule $($rule.Name) (exit code $LASTEXITCODE)."
            }
        }
    }
}

if ($SunshinePath) {
    $resolvedSunshinePath = (Resolve-Path -LiteralPath $SunshinePath -ErrorAction Stop).Path
    $netsh = Resolve-NativeSystemTool "netsh.exe"
    foreach ($protocol in @("TCP", "UDP")) {
        $ruleName = "PeerSpan-Sunshine-LAN-$protocol"
        if ($PSCmdlet.ShouldProcess($ruleName, "Allow bundled Sunshine traffic from the local subnet")) {
            & $netsh advfirewall firewall delete rule "name=$ruleName" | Out-Null
            & $netsh advfirewall firewall add rule `
                "name=$ruleName" `
                "dir=in" `
                "action=allow" `
                "enable=yes" `
                "profile=domain,private" `
                "program=$resolvedSunshinePath" `
                "protocol=$protocol" `
                "remoteip=LocalSubnet" | Out-Null
            if ($LASTEXITCODE -ne 0) {
                throw "netsh failed to create firewall rule $ruleName (exit code $LASTEXITCODE)."
            }
        }
    }
}

Write-Output "VirtualDrivers VDD $($state.driverVersion) is staged. PeerSpan will create the display only while the virtual screen is enabled."
