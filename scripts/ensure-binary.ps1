$ErrorActionPreference = "Stop"

$Repo = "RageLtd/cartographer"
$BinaryName = "cartographer"
$InstallDir = Join-Path $env:USERPROFILE ".cartographer\bin"
$VersionFile = Join-Path $env:USERPROFILE ".cartographer\.version"

# Detect platform
$Arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLower()
if ($Arch -eq "x64") {
    $Platform = "windows-x64"
} else {
    Write-Error "Unsupported architecture: $Arch"
    exit 1
}

$AssetName = "$BinaryName-$Platform.exe"

# Get latest release tag
try {
    $Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ "User-Agent" = "cartographer-installer" }
    $LatestTag = $Release.tag_name
} catch {
    # If we can't reach GitHub but have a binary, use what we have
    $BinaryPath = Join-Path $InstallDir "$BinaryName.exe"
    if (Test-Path $BinaryPath) {
        exit 0
    }
    Write-Error "Cannot determine latest version and no binary installed"
    exit 1
}

# Check if we already have this version
if (Test-Path $VersionFile) {
    $CurrentVersion = Get-Content $VersionFile -Raw
    $CurrentVersion = $CurrentVersion.Trim()
    $BinaryPath = Join-Path $InstallDir "$BinaryName.exe"
    if (($CurrentVersion -eq $LatestTag) -and (Test-Path $BinaryPath)) {
        exit 0
    }
}

Write-Host "Installing $BinaryName $LatestTag ($Platform)..."

# Create install directory
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

# Download binary
$DownloadUrl = "https://github.com/$Repo/releases/download/$LatestTag/$AssetName"
$BinaryPath = Join-Path $InstallDir "$BinaryName.exe"
Invoke-WebRequest -Uri $DownloadUrl -OutFile $BinaryPath -UseBasicParsing

# Save version
$VersionDir = Split-Path $VersionFile -Parent
if (-not (Test-Path $VersionDir)) {
    New-Item -ItemType Directory -Path $VersionDir -Force | Out-Null
}
$LatestTag | Out-File -FilePath $VersionFile -NoNewline -Encoding utf8

Write-Host "$BinaryName $LatestTag installed successfully"
