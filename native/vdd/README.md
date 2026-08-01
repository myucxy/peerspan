# PeerSpan VirtualDrivers VDD integration

PeerSpan ships the unmodified, officially signed `VirtualDrivers/Virtual-Display-Driver`
package from release `25.7.23` (driver `11.30.4.434`). The application creates an
`SWD\MttVDD\PeerSpanVirtualDisplay` software device only while its virtual-screen
lease is active. This uses the upstream INF's `MttVDD` hardware ID and preserves the
catalog signature.

The installer downloads the pinned driver-only archive, verifies SHA-256 and stages
the four signed-release files under `driver/package`. It installs the package in the
Windows driver store and creates `C:\VirtualDisplayDriver\vdd_settings.xml` only when
the machine does not already have one. Ownership is recorded in
`%ProgramData%\PeerSpan\vdd-install-state.json`, so uninstall does not remove a VDD
package, publisher certificate, or configuration that existed before PeerSpan.

For a development-machine installation, first obtain the pinned archive through
`scripts/build-windows-installer.ps1` or extract it manually, then run an elevated
PowerShell after reviewing the script:

```powershell
pwsh -File native\vdd\install.ps1 `
  -PackageDirectory D:\Dev\Env\PeerSpan\runtimes\virtual-display-driver-25.7.23\VirtualDisplayDriver `
  -TrustPublisher -AcknowledgeSystemChanges
```

No VDD source build is required for packaging. The audited upstream source is fixed
as the `third_party/virtual-display-driver` submodule; modifying and rebuilding it
would invalidate the official binary signature and is outside the default release
path.
