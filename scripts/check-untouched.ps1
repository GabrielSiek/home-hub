<#
.SYNOPSIS
  Shows the refactor never touched the existing project: every file outside refactor/ (path, size,
  SHA-256) is recorded once, then compared at each milestone.

.EXAMPLE
  powershell -NoProfile -File refactor\scripts\check-untouched.ps1 -Snapshot   # once, first
  powershell -NoProfile -File refactor\scripts\check-untouched.ps1             # at each milestone

.NOTES
  Build output and tool caches (node_modules, target, dist, gen, .git, *.tsbuildinfo) change on
  their own and are left out. The old data folder (%APPDATA%\dev.tvlauncher.app) isn't checked
  here: the old launcher keeps writing it while you use it. The import proves it never writes there
  by hashing every file it reads before and after.
#>
param([switch]$Snapshot)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$manifest = Join-Path $PSScriptRoot 'untouched.manifest.json'
$skipDirs = @('refactor', 'node_modules', '.git', 'dist', 'target', 'gen')
$skipFiles = @('*.tsbuildinfo')

# Walks the tree without descending into skipped folders (src-tauri\target alone has tens of
# thousands of files).
function Get-ProjectFiles([string]$dir) {
  foreach ($item in Get-ChildItem -LiteralPath $dir -Force) {
    if ($item.PSIsContainer) {
      if ($skipDirs -notcontains $item.Name) { Get-ProjectFiles $item.FullName }
    } elseif (-not ($skipFiles | Where-Object { $item.Name -like $_ })) {
      $item
    }
  }
}

function Get-Tree {
  $tree = [ordered]@{}
  foreach ($f in Get-ProjectFiles $root) {
    $rel = $f.FullName.Substring($root.Length + 1)
    $tree[$rel] = [ordered]@{ size = $f.Length; sha256 = (Get-FileHash -LiteralPath $f.FullName -Algorithm SHA256).Hash }
  }
  $tree
}

$now = Get-Tree

if ($Snapshot) {
  $now | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $manifest -Encoding UTF8
  Write-Output "Snapshot of $($now.Count) files outside refactor\ written to $manifest"
  exit 0
}

if (-not (Test-Path -LiteralPath $manifest)) {
  throw "No snapshot yet: run this script with -Snapshot first."
}

$then = @{}
foreach ($p in (Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json).PSObject.Properties) {
  $then[$p.Name] = $p.Value
}

$problems = @()
foreach ($path in $then.Keys) {
  if (-not $now.Contains($path)) { $problems += "removed  $path" }
  elseif ($now[$path].sha256 -ne $then[$path].sha256) { $problems += "changed  $path" }
}
foreach ($path in $now.Keys) {
  if (-not $then.ContainsKey($path)) { $problems += "added    $path" }
}

# Once the project has commits, git can say the same thing. (Windows PowerShell turns a native
# command's stderr into errors, so they're let through here.)
$ErrorActionPreference = 'Continue'
$null = git -C $root rev-parse --verify --quiet HEAD
if ($LASTEXITCODE -eq 0) {
  $dirty = git -C $root status --porcelain -- . ':(exclude)refactor'
  if ($dirty) { $problems += $dirty | ForEach-Object { "git      $_" } }
}
$ErrorActionPreference = 'Stop'

if ($problems.Count -gt 0) {
  Write-Output "Files outside refactor\ differ from the snapshot:"
  $problems | Sort-Object | ForEach-Object { Write-Output "  $_" }
  exit 1
}
Write-Output "Untouched: all $($then.Count) files outside refactor\ match the snapshot."
