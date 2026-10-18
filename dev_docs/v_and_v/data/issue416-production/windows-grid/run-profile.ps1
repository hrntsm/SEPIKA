param(
    [Parameter(Mandatory=$true)][string]$OutputName,
    [string]$Mode = 'grid',
    [string[]]$ModeArgs = @()
)
$ErrorActionPreference = 'Stop'
$workspace = 'C:\Users\hiroa\Desktop\ikkan'
$gridRoot = Join-Path $env:TEMP 'sepika416-production-grid'
$outputPath = Join-Path $gridRoot $OutputName
if (Test-Path -LiteralPath $outputPath) { throw 'OutputName already exists' }
New-Item -ItemType Directory -Path $outputPath | Out-Null
$exePath = Join-Path $workspace 'target\release\examples\cft_torsion_profile.exe'
if (!(Test-Path -LiteralPath $exePath)) { throw 'release example executable missing' }
$preflight = [ordered]@{
    start_utc = [DateTime]::UtcNow.ToString('o')
    os_cold_reset = $false
    compile_excluded = $true
    os = Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,BuildNumber,OSArchitecture,TotalVisibleMemorySize,FreePhysicalMemory
    cpu = @(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors,MaxClockSpeed)
    computer = Get-CimInstance Win32_ComputerSystem | Select-Object TotalPhysicalMemory
    rustc = (& rustc -Vv | Out-String)
    executable_sha256 = (Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash
    memory_contract = 'Maximum observed OS PeakWorkingSet64 and PeakPagedMemorySize64, polled every 100ms while alive. These are process-lifetime high-water marks as observed, not case increments or factor fill. The last sub-100ms interval is not guaranteed sampled.'
}
$sourcePaths = @('Cargo.lock','Cargo.toml','crates/sepika-core/Cargo.toml','crates/sepika-core/examples/cft_torsion_profile.rs')
$sourcePaths += @(Get-ChildItem -LiteralPath (Join-Path $workspace 'crates/sepika-core/src/section_shape') -File -Recurse | ForEach-Object { [IO.Path]::GetRelativePath($workspace, $_.FullName) })
$preflight.source_sha256 = @($sourcePaths | ForEach-Object { [ordered]@{ path=$_; sha256=(Get-FileHash -LiteralPath (Join-Path $workspace $_) -Algorithm SHA256).Hash } })
$preflight | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $outputPath 'environment.json') -Encoding utf8
$quotedOutput = '"' + $outputPath + '"'
$exeArgs = @($quotedOutput, $Mode) + $ModeArgs
$watch = [Diagnostics.Stopwatch]::StartNew()
$process = Start-Process -FilePath $exePath -ArgumentList $exeArgs -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $outputPath 'stdout.txt') -RedirectStandardError (Join-Path $outputPath 'stderr.txt')
$peakWorkingSet = 0L
$peakPagedMemory = 0L
$cpuSeconds = 0.0
while (!$process.WaitForExit(100)) {
    $process.Refresh()
    $peakWorkingSet = [Math]::Max($peakWorkingSet, $process.PeakWorkingSet64)
    $peakPagedMemory = [Math]::Max($peakPagedMemory, $process.PeakPagedMemorySize64)
    $cpuSeconds = [Math]::Max($cpuSeconds, $process.TotalProcessorTime.TotalSeconds)
}
$watch.Stop()
$process.Refresh()
[ordered]@{
    pid=$process.Id
    exit_code=$process.ExitCode
    end_utc=[DateTime]::UtcNow.ToString('o')
    external_process_wall_seconds=$watch.Elapsed.TotalSeconds
    peak_working_set_bytes=$peakWorkingSet
    peak_paged_memory_bytes=$peakPagedMemory
    observed_total_processor_seconds=$cpuSeconds
    memory_contract=$preflight.memory_contract
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $outputPath 'process.json') -Encoding utf8
if ($process.ExitCode -ne 0) { throw "Runner exited $($process.ExitCode)" }

