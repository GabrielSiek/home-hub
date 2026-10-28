# Sets the server laptop up to deploy itself on each push to main (.github/workflows/deploy-server.yml).
# Run once, from a PowerShell opened as administrator, after the server is in
# C:\Program Files\TVLauncher and the household is imported (docs/OPERATIONS.md, steps 1-3):
#
#   1. Installs the tvl-server service, if it isn't yet
#   2. Lets the runner's account (NETWORK SERVICE) replace the files in C:\Program Files\TVLauncher
#      and stop/start the tvl-server service: nothing more. It can't read the data folder or its keys.
#   3. Downloads the GitHub Actions runner (checksum verified) into C:\actions-runner and registers it
#      with the repo as a Windows service, labelled home-hub
#
# The token comes from the repo's Settings > Actions > Runners > New self-hosted runner, or:
#   gh api -X POST repos/<owner>/<repo>/actions/runners/registration-token --jq .token

param(
  [Parameter(Mandatory)][string]$Repo,
  [Parameter(Mandatory)][string]$Token,
  [string]$RunnerDir = 'C:\actions-runner',
  [string]$Name = $env:COMPUTERNAME
)

$ErrorActionPreference = 'Stop'
$service = 'tvl-server'
$app = Join-Path $env:ProgramFiles 'TVLauncher'
$exe = Join-Path $app 'tvl-server.exe'
$networkService = '*S-1-5-20'

function Step($text) { Write-Host ''; Write-Host "== $text" -ForegroundColor Cyan }

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) { throw 'Run this from a PowerShell opened as administrator.' }
if (-not (Test-Path -LiteralPath $exe)) { throw "No ${exe}: copy the server there first (docs/OPERATIONS.md)." }

Step "The $service service"
if (Get-Service $service -ErrorAction SilentlyContinue) {
  Write-Host '  Already installed.'
} else {
  & $exe install
  if ($LASTEXITCODE -ne 0) { throw 'tvl-server install failed (see above).' }
}

Step "Letting the runner replace the server in $app"
icacls $app /grant "${networkService}:(OI)(CI)M" | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'icacls failed.' }

Step "Letting the runner stop and start $service"
# Start, stop, query status, interrogate, read permissions; for NETWORK SERVICE (NS).
$ace = '(A;;RPWPLCLORC;;;NS)'
$sd = (sc.exe sdshow $service | Where-Object { $_ }) -join ''
if ($sd -like "*$ace*") {
  Write-Host '  Already allowed.'
} else {
  # The ACE goes at the end of the D: (access) part, before any S: (audit) part.
  $new = if ($sd.Contains('S:')) { ([regex]'S:').Replace($sd, "${ace}S:", 1) } else { $sd + $ace }
  sc.exe sdset $service $new | Out-Null
  if ($LASTEXITCODE -ne 0) { throw 'sc sdset failed.' }
}

Step "The GitHub Actions runner in $RunnerDir"
$release = Invoke-RestMethod 'https://api.github.com/repos/actions/runner/releases/latest' -UseBasicParsing
$version = $release.tag_name.TrimStart('v')
$zip = "actions-runner-win-x64-$version.zip"
if (-not ($release.body -match '<!-- BEGIN SHA win-x64 -->([0-9a-f]{64})<!-- END SHA win-x64 -->')) { throw "No checksum for $zip in the release notes." }
$sha = $Matches[1]
if (Test-Path (Join-Path $RunnerDir '.runner')) { throw "$RunnerDir already holds a registered runner: remove it first (config.cmd remove)." }
New-Item -ItemType Directory -Force $RunnerDir | Out-Null
$download = Join-Path $env:TEMP $zip
Invoke-WebRequest "https://github.com/actions/runner/releases/download/v$version/$zip" -OutFile $download -UseBasicParsing
if ((Get-FileHash $download -Algorithm SHA256).Hash -ne $sha) { throw "$zip doesn't match its published checksum." }
Expand-Archive $download $RunnerDir -Force
Remove-Item $download
Write-Host "  Runner $version"

Step 'Registering it with GitHub, as a Windows service (NETWORK SERVICE)'
Push-Location $RunnerDir
try {
  .\config.cmd --unattended --url $Repo --token $Token --name $Name --labels home-hub --runasservice --replace
  if ($LASTEXITCODE -ne 0) { throw 'config.cmd failed (see above).' }
} finally { Pop-Location }

Write-Host ''
Write-Host 'Done. Each push to main that touches the server or the companion now deploys here.' -ForegroundColor Green
