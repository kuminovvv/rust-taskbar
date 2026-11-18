# Read-only diagnostics for Windows Rust/MSVC prerequisites.
# Run: pwsh -NoProfile -File .\scripts\check-toolchain.ps1
# No installation, registry changes, or environment mutation.

$ErrorActionPreference = 'Stop'
Write-Host "rust-taskbar: Windows toolchain preflight"
Write-Host "--------------------------------------"

$allOK = $true

foreach ($name in @('rustc', 'cargo', 'rustup')) {
    $command = Get-Command $name -ErrorAction SilentlyContinue
    if ($null -eq $command) {
        Write-Warning "$name is not available in PATH"
        $allOK = $false
    } else {
        Write-Host "$name => $($command.Source)"
    }
}

if ((Get-Command rustup -ErrorAction SilentlyContinue)) {
    $active = (& rustup show active-toolchain 2>&1 | Out-String).Trim()
    Write-Host "active toolchain: $active"
    if ($LASTEXITCODE -ne 0 -or $active -notmatch 'x86_64-pc-windows-msvc') {
        Write-Warning "For this project, use an x86_64-pc-windows-msvc Rust toolchain."
        $allOK = $false
    }
}

$pf86 = [Environment]::GetFolderPath('ProgramFilesX86')
if ([string]::IsNullOrWhiteSpace($pf86)) {
    Write-Warning "Cannot determine Program Files (x86)."
    exit 1
}

$vswhere = Join-Path $pf86 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere)) {
    Write-Warning "vswhere.exe not found. Install Visual Studio 2022 Build Tools with Desktop development with C++."
    exit 1
}

# Require the compiler/linker component, not merely the VS installer shell.
$installPath = (& $vswhere -latest -products '*' -version '[17.0,18.0)' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>&1 | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($installPath) -or -not (Test-Path -LiteralPath $installPath)) {
    Write-Warning "Visual Studio 2022 C++ x64/x86 build tools component not found."
    Write-Host "Open Visual Studio Installer -> Build Tools 2022 -> Modify -> Desktop development with C++."
    Write-Host "Include MSVC v143 x64/x86 and Windows 10/11 SDK."
    exit 1
}

Write-Host "MSVC installation: $installPath"

$linkerPattern = Join-Path $installPath 'VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe'
$link = Get-ChildItem -Path $linkerPattern -ErrorAction SilentlyContinue | Select-Object -First 1
if ($null -eq $link) {
    Write-Warning "MSVC component reported installed but Hostx64/x64/link.exe not found."
    $allOK = $false
} else {
    Write-Host "link.exe => $($link.FullName)"
}

$sdkLibs = Join-Path $pf86 'Windows Kits\10\Lib'
if (-not (Test-Path -LiteralPath $sdkLibs)) {
    Write-Warning "Windows SDK libraries not found under $sdkLibs"
    $allOK = $false
} else {
    Write-Host "Windows SDK libraries => $sdkLibs"
}

$vsDevCmd = Join-Path $installPath 'Common7\Tools\VsDevCmd.bat'
if (Test-Path -LiteralPath $vsDevCmd) {
    Write-Host "Developer command environment => $vsDevCmd"
}

if (-not $allOK) {
    Write-Warning "Toolchain incomplete. Resolve the warnings before running cargo."
    exit 1
}

Write-Host "Toolchain components found. If normal PowerShell still reports link.exe missing, use Developer PowerShell for VS 2022."
Write-Host "Then: cargo test --all-targets; cargo run --release"
exit 0
