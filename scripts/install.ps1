$ErrorActionPreference = "Stop"

$version = if ($env:WAYLOG_VERSION) { $env:WAYLOG_VERSION } else { "latest" }
$installDir = if ($env:WAYLOG_INSTALL_DIR) { $env:WAYLOG_INSTALL_DIR } else { Join-Path $HOME ".local\bin" }

$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
switch ($architecture) {
    "Arm64" { $target = "aarch64-pc-windows-msvc" }
    "X64" { $target = "x86_64-pc-windows-msvc" }
    default { throw "Unsupported Windows architecture: $architecture" }
}

$archive = "waylog-$target.zip"
if ($version -eq "latest") {
    $baseUrl = "https://github.com/soaringk/waylog-cli/releases/latest/download"
} else {
    $tag = if ($version.StartsWith("v")) { $version } else { "v$version" }
    $baseUrl = "https://github.com/soaringk/waylog-cli/releases/download/$tag"
}

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("waylog-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tempDir | Out-Null

try {
    $archivePath = Join-Path $tempDir $archive
    $checksumPath = "$archivePath.sha256"
    Write-Host "Downloading $archive..."
    Invoke-WebRequest -Uri "$baseUrl/$archive" -OutFile $archivePath
    Invoke-WebRequest -Uri "$baseUrl/$archive.sha256" -OutFile $checksumPath

    $expected = (Get-Content -Raw $checksumPath).Trim().ToLowerInvariant()
    $actual = (Get-FileHash -Algorithm SHA256 $archivePath).Hash.ToLowerInvariant()
    if ($expected -ne $actual) {
        throw "Checksum verification failed for $archive."
    }

    Expand-Archive -Path $archivePath -DestinationPath $tempDir
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    Copy-Item -Force (Join-Path $tempDir "waylog.exe") (Join-Path $installDir "waylog.exe")

    Write-Host "Installed waylog to $(Join-Path $installDir 'waylog.exe')"
    & (Join-Path $installDir "waylog.exe") --version
    if (($env:PATH -split ';') -notcontains $installDir) {
        Write-Host "Add $installDir to PATH to run waylog directly."
    }
} finally {
    Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
}
