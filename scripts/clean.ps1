# Clean generated project files without touching source or user-wide caches.
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [switch]$IncludeLocalCache
)

$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (-not (Test-Path -LiteralPath (Join-Path $projectRoot 'Cargo.toml') -PathType Leaf)) {
    throw 'Run this script from the ClipForge project.'
}

$cleanupNames = @('target', 'dist', 'gen')
if ($IncludeLocalCache) {
    $cleanupNames += '.cargo-local'
}

foreach ($cleanupName in $cleanupNames) {
    $cleanupPath = [IO.Path]::GetFullPath((Join-Path $projectRoot $cleanupName))
    if (-not $cleanupPath.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Cleanup path is outside the project: $cleanupPath"
    }
    if (-not (Test-Path -LiteralPath $cleanupPath)) {
        continue
    }
    $cleanupItem = Get-Item -LiteralPath $cleanupPath -Force
    if ($cleanupItem.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw "Refusing to clean a symbolic link or junction: $cleanupPath"
    }
    $nestedLinks = Get-ChildItem -LiteralPath $cleanupPath -Recurse -Force |
        Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint } |
        Select-Object -First 1
    if ($nestedLinks) {
        throw "Refusing to clean a directory containing a symbolic link or junction: $cleanupPath"
    }
    if ($PSCmdlet.ShouldProcess($cleanupPath, 'Remove generated files')) {
        Remove-Item -LiteralPath $cleanupPath -Recurse
        Write-Output "Removed $cleanupName"
    }
}
