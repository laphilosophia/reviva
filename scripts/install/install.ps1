param(
    [string]$Version = $(if ($env:REVIVA_VERSION) { $env:REVIVA_VERSION } else { "latest" }),
    [string]$Repo = $(if ($env:REVIVA_REPO) { $env:REVIVA_REPO } else { "laphilosophia/reviva" }),
    [string]$BinDir = $(if ($env:REVIVA_BIN_DIR) { $env:REVIVA_BIN_DIR } else { Join-Path $HOME ".reviva\\bin" })
)

$ErrorActionPreference = "Stop"

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
if ($arch -ne "x64") {
    throw "unsupported Windows architecture: $arch (supported: x64)"
}

$assetName = "reviva-windows-x86_64.zip"
if ($Version -eq "latest") {
    $url = "https://github.com/$Repo/releases/latest/download/$assetName"
}
else {
    $url = "https://github.com/$Repo/releases/download/$Version/$assetName"
}

$tempRoot = New-Item -Path ([System.IO.Path]::GetTempPath()) -Name ("reviva-install-" + [System.Guid]::NewGuid().ToString("N")) -ItemType Directory
$archive = Join-Path $tempRoot.FullName $assetName
$extractDir = Join-Path $tempRoot.FullName "extract"

try {
    Write-Host "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $archive

    New-Item -ItemType Directory -Force -Path $extractDir | Out-Null
    Expand-Archive -Path $archive -DestinationPath $extractDir -Force

    New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
    Copy-Item -Path (Join-Path $extractDir "reviva.exe") -Destination (Join-Path $BinDir "reviva.exe") -Force
    Write-Host "Installed: $(Join-Path $BinDir 'reviva.exe')"

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ([string]::IsNullOrWhiteSpace($userPath)) {
        $userPath = $BinDir
    }
    elseif (-not ($userPath.Split(';') -contains $BinDir)) {
        $userPath = "$userPath;$BinDir"
    }
    [Environment]::SetEnvironmentVariable("Path", $userPath, "User")

    Write-Host "PATH updated for future terminals (User scope)."
    Write-Host "Run in a new terminal: reviva --help"
}
finally {
    if (Test-Path $tempRoot.FullName) {
        Remove-Item -Recurse -Force $tempRoot.FullName
    }
}
