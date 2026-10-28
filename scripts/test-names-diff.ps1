<#
.SYNOPSIS
  Lists the Rust tests of the old src-tauri\src that refactor\ doesn't have yet. Tests move with
  their modules, so once a milestone is done every old name should be in the new tree.

.EXAMPLE
  powershell -NoProfile -File refactor\scripts\test-names-diff.ps1           # progress report
  powershell -NoProfile -File refactor\scripts\test-names-diff.ps1 -Strict   # fails on a gap

.NOTES
  Reads the sources (#[test] / #[tokio::test] and the fn after it); nothing is built. Names are
  counted, so a name used by two modules must be there twice.
#>
param([switch]$Strict)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Get-TestNames([string[]]$dirs) {
  $count = @{}
  foreach ($dir in $dirs) {
    if (-not (Test-Path -LiteralPath $dir)) { continue }
    foreach ($file in Get-ChildItem -LiteralPath $dir -Recurse -Filter *.rs -File) {
      $pending = $false
      foreach ($line in [IO.File]::ReadLines($file.FullName)) {
        if ($line -match '^\s*#\[(tokio::)?test\b') { $pending = $true; continue }
        if ($pending -and $line -match '^\s*(pub\s+)?(async\s+)?fn\s+([A-Za-z0-9_]+)') {
          $name = $Matches[3]
          $count[$name] = 1 + [int]$count[$name]
          $pending = $false
        }
      }
    }
  }
  $count
}

$old = Get-TestNames @(Join-Path $root 'src-tauri\src')
$new = Get-TestNames @((Join-Path $root 'refactor\crates'), (Join-Path $root 'refactor\apps\tv\src-tauri\src'))

$oldTotal = ($old.Values | Measure-Object -Sum).Sum
$missing = @()
foreach ($name in ($old.Keys | Sort-Object)) {
  $gap = $old[$name] - [int]$new[$name]
  if ($gap -gt 0) { $missing += $(if ($gap -gt 1) { "$name (x$gap)" } else { $name }) }
}
$newOnly = ($new.Keys | Where-Object { -not $old.ContainsKey($_) }).Count

Write-Output ("Old tests: {0}. Carried over: {1}. New tests in refactor\: {2}." -f $oldTotal, ($oldTotal - ($missing | ForEach-Object { if ($_ -match '\(x(\d+)\)$') { [int]$Matches[1] } else { 1 } } | Measure-Object -Sum).Sum), $newOnly)
if ($missing.Count -gt 0) {
  Write-Output "Not in refactor\ yet ($($missing.Count) names):"
  $missing | ForEach-Object { Write-Output "  $_" }
  if ($Strict) { exit 1 }
}
