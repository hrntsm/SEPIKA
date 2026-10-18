$ErrorActionPreference = 'Stop'
$calibrationRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$calibrationLog = Get-Content -Raw -LiteralPath (Join-Path $calibrationRoot 'final-calibration-run.log')
$calibrationDirectories = @([regex]::Matches($calibrationLog, 'calibration evidence: ([^\r\n]+)') | ForEach-Object { $_.Groups[1].Value.Trim() })
if ($calibrationDirectories.Count -ne 4) { throw '元4testの出力directoryが揃いません' }
$calibrationLevels = @()
$calibrationSummary = [ordered]@{directories=$calibrationDirectories;tests_passed=($calibrationLog -match 'test result: ok\. 4 passed; 0 failed');runs=@()}
foreach ($calibrationDirectory in $calibrationDirectories) {
    $calibrationRecords = @(Get-Content -LiteralPath (Join-Path $calibrationDirectory 'rows.jsonl') | ForEach-Object { $_ | ConvertFrom-Json })
    foreach ($calibrationRecord in $calibrationRecords) {
        if ($calibrationRecord.level) {
            $calibrationLevel = $calibrationRecord.level
            $calibrationLevel | Add-Member -NotePropertyName run -NotePropertyValue (Split-Path -Leaf $calibrationDirectory)
            $calibrationLevel | Add-Member -NotePropertyName ratio -NotePropertyValue $calibrationRecord.ratio
            $calibrationLevel | Add-Member -NotePropertyName label -NotePropertyValue $calibrationRecord.label
            $calibrationLevel | Add-Member -NotePropertyName name -NotePropertyValue $calibrationRecord.name
            $calibrationLevels += $calibrationLevel
        }
    }
    $calibrationSummary.runs += [ordered]@{
        directory=$calibrationDirectory
        levels=@($calibrationRecords | Where-Object {$_.level}).Count
        mesh_files=@(Get-ChildItem -LiteralPath $calibrationDirectory -Filter '*.json').Count
        summaries=@($calibrationRecords | Where-Object {!$_.level})
    }
}
$calibrationLevels | Export-Csv -NoTypeInformation -LiteralPath (Join-Path $calibrationRoot 'final-calibration-levels.csv')
$calibrationSummary.level_count = $calibrationLevels.Count
$calibrationSummary.max_relative_residual = ($calibrationLevels | Measure-Object -Property relative_residual -Maximum).Maximum
$calibrationSummary.max_vertices = ($calibrationLevels | Measure-Object -Property vertices -Maximum).Maximum
$calibrationSummary.max_triangles = ($calibrationLevels | Measure-Object -Property triangles -Maximum).Maximum
$calibrationSummary.max_free_dofs = ($calibrationLevels | Measure-Object -Property free_dofs -Maximum).Maximum
$calibrationSummary.max_nnz = ($calibrationLevels | Measure-Object -Property nnz -Maximum).Maximum
$calibrationSummary.all_levels_quality = @($calibrationLevels | Where-Object {
    $_.relative_residual -gt 1e-10 -or $_.vertices -gt 200000 -or $_.triangles -gt 400000 -or
    $_.free_dofs -le 0 -or $_.nnz -lt $_.free_dofs -or $_.j_hat -le 0 -or
    $_.min_signed_double_area -le 0 -or $_.min_angle_rad -le 0 -or
    $_.min_area_edge_ratio -le (128*2.2204460492503131e-16) -or $_.max_actual_area -gt $_.area_limit*(1+128*2.2204460492503131e-16)
}).Count -eq 0
$calibrationSummary | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $calibrationRoot 'final-calibration-summary.json')
$calibrationArtifacts = @()
foreach ($calibrationDirectory in $calibrationDirectories) {
    $calibrationArtifacts += Get-ChildItem -LiteralPath $calibrationDirectory -File | ForEach-Object {
        [PSCustomObject]@{path=$_.FullName;bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash}
    }
}
$calibrationArtifacts | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $calibrationRoot 'final-calibration-raw-sha256.json')
[PSCustomObject]$calibrationSummary | Select-Object tests_passed,level_count,max_relative_residual,max_vertices,max_triangles,max_free_dofs,max_nnz,all_levels_quality | Format-List
if (!$calibrationSummary.tests_passed -or !$calibrationSummary.all_levels_quality) { throw '校正または保存level品質が未達' }
