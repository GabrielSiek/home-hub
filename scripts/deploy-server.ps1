# Puts a new build of the server into C:\Program Files\TVLauncher and restarts the service. The
# deploy workflow runs it on the server laptop; by hand (admin): deploy-server.ps1 -From <folder>
# with tvl-server.exe and companion\ in it.
#
# The data folder (C:\ProgramData\TVLauncher\server) is never touched. The build that was running
# is kept in TVLauncher\previous; if the new one doesn't answer within a minute, it goes back.

param(
  [Parameter(Mandatory)][string]$From,
  [string]$Commit = ''
)

$ErrorActionPreference = 'Stop'
$service = 'tvl-server'
$app = Join-Path $env:ProgramFiles 'TVLauncher'
$previous = Join-Path $app 'previous'
$parts = @('tvl-server.exe', 'companion')

function Step($text) { Write-Host "== $text" }

foreach ($p in $parts) {
  if (-not (Test-Path -LiteralPath (Join-Path $From $p))) { throw "No $p in $From" }
}
if (-not (Get-Service $service -ErrorAction SilentlyContinue)) {
  throw "No $service service: install it first (tvl-server install, from an administrator console)"
}

function Stop-Server {
  Stop-Service $service
  (Get-Service $service).WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
}

# Copies retry for a few seconds: Windows can hold the exe for a moment after the service stops.
function Copy-Parts($src, $dst) {
  foreach ($p in $parts) {
    $target = Join-Path $dst $p
    for ($try = 1; ; $try++) {
      try {
        if (Test-Path -LiteralPath $target) { Remove-Item -LiteralPath $target -Recurse -Force }
        Copy-Item -LiteralPath (Join-Path $src $p) $target -Recurse -Force
        break
      } catch {
        if ($try -ge 10) { throw }
        Start-Sleep -Seconds 1
      }
    }
  }
}

# Up, with its data loaded (/ca.crt) and the companion's page served (/).
function Test-Server {
  for ($i = 0; $i -lt 30; $i++) {
    Start-Sleep -Seconds 2
    if ((Get-Service $service).Status -ne 'Running') { continue }
    try {
      $ca = Invoke-WebRequest 'http://localhost:47960/ca.crt' -UseBasicParsing -TimeoutSec 3
      $page = Invoke-WebRequest 'http://localhost:47960/' -UseBasicParsing -TimeoutSec 3
      if ($ca.StatusCode -eq 200 -and $page.StatusCode -eq 200) { return $true }
    } catch {}
  }
  return $false
}

Step "Stopping $service"
Stop-Server

Step "Keeping the running build in $previous"
New-Item -ItemType Directory -Force $previous | Out-Null
Copy-Parts $app $previous

Step "Copying the new build$(if ($Commit) { " ($Commit)" }) into $app"
Copy-Parts $From $app
if ($Commit) { Set-Content -LiteralPath (Join-Path $app 'deployed.txt') -Value "$Commit $(Get-Date -Format o)" }

Step "Starting $service"
Start-Service $service
if (Test-Server) {
  Step 'Deployed: the server answers on http://localhost:47960'
  exit 0
}

Write-Host "The new build didn't come up: putting the previous one back. See C:\ProgramData\TVLauncher\server\logs."
Stop-Server
Copy-Parts $previous $app
Start-Service $service
if (Test-Server) { Write-Host 'The previous build is running again.' } else { Write-Host 'The previous build did not come up either.' }
exit 1
