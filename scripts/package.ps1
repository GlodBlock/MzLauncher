param(
    [string]$Target = "",
    [string]$OutDir = "dist"
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot/.."
Push-Location $root
try {
    $buildArgs = @("build", "--release", "--workspace")
    if ($Target) { $buildArgs += @("--target", $Target) }
    cargo @buildArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

    $binDir = if ($Target) { "target/$Target/release" } else { "target/release" }
    $platform = if ($Target) {
        $Target
    } else {
        (rustc -vV | Select-String '^host:').ToString().Substring(6).Trim()
    }
    $version = (Select-String -Path "core/Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
    $exe = if ($platform -like "*windows*") { ".exe" } else { "" }

    $packages = @(
        @{ Name = "core"; Files = @("mzlauncher-core$exe", "LICENSE") },
        @{ Name = "gui"; Files = @("mzlauncher-gui$exe", "LICENSE") },
        @{ Name = "full"; Files = @("mzlauncher-gui$exe", "mzlauncher-core$exe", "LICENSE", "README.md") }
    )

    New-Item -ItemType Directory -Force $OutDir | Out-Null
    foreach ($package in $packages) {
        $staging = "target/package/$($package.Name)"
        Remove-Item -Recurse -Force $staging -ErrorAction SilentlyContinue
        New-Item -ItemType Directory -Force $staging | Out-Null
        foreach ($file in $package.Files) {
            $source = if ($file -like "mzlauncher-*") { "$binDir/$file" } else { $file }
            if (-not (Test-Path $source)) { throw "missing build artifact: $source" }
            Copy-Item $source $staging
        }
        $archive = "$OutDir/mzlauncher-$($package.Name)-$version-$platform.zip"
        Remove-Item -Force $archive -ErrorAction SilentlyContinue
        Compress-Archive -Path "$staging/*" -DestinationPath $archive
        Write-Host "created $archive"
    }
} finally {
    Pop-Location
}
