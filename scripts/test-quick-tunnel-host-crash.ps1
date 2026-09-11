param([Parameter(Mandatory=$true)][string]$TestExecutable)
$ErrorActionPreference = 'Stop'
$binary = (Resolve-Path -LiteralPath $TestExecutable).Path
$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('serena-quick-crash-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $testRoot | Out-Null
$previousDirectory = $env:SERENA_QUICK_CRASH_DIRECTORY
$testHost = $null
try {
    $env:SERENA_QUICK_CRASH_DIRECTORY = $testRoot
    $testHost = Start-Process -FilePath $binary -ArgumentList @('--exact', 'remote::quick_tunnel::host_crash_tests::isolated_quick_tunnel_host', '--ignored', '--nocapture') -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $testRoot 'stdout.log') -RedirectStandardError (Join-Path $testRoot 'stderr.log')
    $deadline = [DateTime]::UtcNow.AddSeconds(200)
    $readyPath = Join-Path $testRoot 'owned.json'
    while (!(Test-Path -LiteralPath $readyPath)) {
        if ($testHost.HasExited) { throw "Isolated Host exited before ownership marker; evidence: $testRoot" }
        if ([DateTime]::UtcNow -gt $deadline) { throw "Quick Tunnel ownership timeout; evidence: $testRoot" }
        Start-Sleep -Milliseconds 250
    }
    $ready = Get-Content -LiteralPath $readyPath -Raw | ConvertFrom-Json
    if ($ready.hostPid -ne $testHost.Id) { throw 'Host identity mismatch' }
    $tunnel = Get-Process -Id $ready.cloudflaredPid
    $process = Get-CimInstance Win32_Process -Filter "ProcessId = $($tunnel.Id)"
    if ($process.ParentProcessId -ne $testHost.Id) { throw 'cloudflared owner mismatch' }
    $beforeTcp = @(Get-NetTCPConnection -OwningProcess $tunnel.Id -ErrorAction SilentlyContinue)
    $beforeUdp = @(Get-NetUDPEndpoint -OwningProcess $tunnel.Id -ErrorAction SilentlyContinue)
    if (($beforeTcp.Count + $beforeUdp.Count) -eq 0) { throw 'cloudflared has no live sockets before Host kill' }
    # Only this isolated test Host is terminated. No tree kill / remote_stop.
    Stop-Process -Id $testHost.Id -Force
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while (!$tunnel.HasExited -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 100 }
    if (!$tunnel.HasExited) { throw 'FAIL: cloudflared survived Host TerminateProcess' }
    $afterTcp = @(Get-NetTCPConnection -OwningProcess $ready.cloudflaredPid -ErrorAction SilentlyContinue)
    $afterUdp = @(Get-NetUDPEndpoint -OwningProcess $ready.cloudflaredPid -ErrorAction SilentlyContinue)
    if ($afterTcp.Count -or $afterUdp.Count) { throw 'FAIL: cloudflared sockets survived Host exit' }
    $result = [ordered]@{ status='PASS'; hostPid=$ready.hostPid; cloudflaredPid=$ready.cloudflaredPid; beforeTcp=$beforeTcp.Count; beforeUdp=$beforeUdp.Count; afterTcp=$afterTcp.Count; afterUdp=$afterUdp.Count; evidence=$testRoot; scope='production Remote/Broker/cloudflared in isolated native test Host; not a Tauri UI test' }
    $result | ConvertTo-Json | Tee-Object -FilePath (Join-Path $testRoot 'result.json')
} finally {
    if ($null -ne $testHost -and !$testHost.HasExited) { Stop-Process -Id $testHost.Id -Force }
    $env:SERENA_QUICK_CRASH_DIRECTORY = $previousDirectory
}
