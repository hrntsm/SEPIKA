param([string]$OutputPath)

$ErrorActionPreference = 'Stop'
$pipeName = 'sepika-973-' + [Guid]::NewGuid().ToString('N')
$server = Start-Job -ArgumentList $pipeName -ScriptBlock {
    param($Name)
    $ErrorActionPreference = 'Stop'
    $options = [IO.Pipes.PipeOptions]::Asynchronous -bor [IO.Pipes.PipeOptions]::CurrentUserOnly
    $pipe = [IO.Pipes.NamedPipeServerStream]::new($Name, [IO.Pipes.PipeDirection]::InOut, 1,
        [IO.Pipes.PipeTransmissionMode]::Byte, $options)
    try {
        'ready'
        if (-not $pipe.WaitForConnectionAsync().Wait(15000)) { throw 'server connection timeout' }
        $reader = [IO.StreamReader]::new($pipe)
        $writer = [IO.StreamWriter]::new($pipe)
        $writer.AutoFlush = $true
        $request = $reader.ReadLine() | ConvertFrom-Json
        if ($request.method -ne 'study.get') { throw 'unexpected method' }
        $writer.WriteLine((@{id=$request.id; study_id=$request.study_id; status='observed'} | ConvertTo-Json -Compress))
        $pipe.Dispose()
    } finally { $pipe.Dispose() }
}

try {
    $ready = $null
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ($ready -ne 'ready' -and [DateTime]::UtcNow -lt $deadline) {
        $ready = Receive-Job $server
        if ($server.State -eq 'Failed') { throw 'server failed before ready' }
        Start-Sleep -Milliseconds 50
    }
    if ($ready -ne 'ready') { throw 'server readiness timeout' }
    $client = [IO.Pipes.NamedPipeClientStream]::new('.', $pipeName, [IO.Pipes.PipeDirection]::InOut,
        [IO.Pipes.PipeOptions]::Asynchronous)
    $client.Connect(3000)
    $reader = [IO.StreamReader]::new($client)
    $writer = [IO.StreamWriter]::new($client)
    $writer.AutoFlush = $true
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $writer.WriteLine('{"id":"r1","method":"study.get","study_id":"s1"}')
    $response = $reader.ReadLineAsync()
    if (-not $response.Wait(3000)) { throw 'response timeout' }
    $reply = $response.Result | ConvertFrom-Json
    $watch.Stop()
    if ($reply.id -ne 'r1' -or $reply.study_id -ne 's1' -or $reply.status -ne 'observed') { throw 'roundtrip mismatch' }
    $eof = $reader.ReadLineAsync()
    if (-not $eof.Wait(3000)) { throw 'disconnect detection timeout' }
    if ($null -ne $eof.Result) { throw 'expected EOF' }
    $client.Dispose()
    $null = Wait-Job $server -Timeout 5
    Receive-Job $server -ErrorAction Stop | Out-Null
    $absent = [IO.Pipes.NamedPipeClientStream]::new('.', $pipeName, [IO.Pipes.PipeDirection]::InOut)
    $timeoutObserved = $false
    try { $absent.Connect(300) } catch [TimeoutException] { $timeoutObserved = $true } finally { $absent.Dispose() }
    if (-not $timeoutObserved) { throw 'expected absent-server timeout' }
    $record = [ordered]@{
        utc=[DateTime]::UtcNow.ToString('o'); os=[Environment]::OSVersion.VersionString
        powershell=$PSVersionTable.PSVersion.ToString(); dotnet=[Environment]::Version.ToString()
        transport='Windows named pipe / byte / CurrentUserOnly'; roundtrip='pass'
        response_id=$reply.id; study_id=$reply.study_id; roundtrip_ms=$watch.Elapsed.TotalMilliseconds
        server_disconnect_eof='pass'; absent_server_timeout='pass'
        product_gui='not tested'; mcp_client='not tested'; cross_user_denial='not tested'
    }
    $json = $record | ConvertTo-Json
    if ($OutputPath) { Set-Content -LiteralPath $OutputPath -Value $json -Encoding utf8 }
    $json
} finally {
    if ($client) { $client.Dispose() }
    Stop-Job $server
    Remove-Job $server -Force
}
