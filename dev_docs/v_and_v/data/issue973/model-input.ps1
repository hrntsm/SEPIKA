param([string]$OutputPath)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
$source = Join-Path $repo 'crates/sepika-app/tests/fixtures/model.stb'
[xml]$xml = Get-Content -LiteralPath $source
$target = @($xml.SelectNodes('//*[local-name()="StbGirder" and @id="43"]'))
if ($target.Count -ne 1 -or $target[0].id_section -ne '10' -or $target[0].kind_structure -ne 'S') { throw 'target changed' }
$sections = foreach ($sectionId in '10','11','23') {
    $section = @($xml.SelectNodes("//*[local-name()='StbSecBeam_S' and @id='$sectionId']"))
    if ($section.Count -ne 1 -or $section[0].kind_beam -ne 'GIRDER') { throw 'section changed' }
    $shape = $section[0].StbSecSteelFigureBeam_S.StbSecSteelBeam_S_Straight
    [ordered]@{source_id=$sectionId; name=$section[0].name; floor=$section[0].floor; shape=$shape.shape; steel=$shape.strength_main}
}
$nodes = foreach ($nodeId in '13','14') {
    $node = @($xml.SelectNodes("//*[local-name()='StbNode' and @id='$nodeId']"))
    if ($node.Count -ne 1) { throw 'node changed' }
    [ordered]@{source_id=$nodeId; xyz_mm=@([double]$node[0].X,[double]$node[0].Y,[double]$node[0].Z)}
}
$manifest = [ordered]@{
    base_commit='1b057e81'; fixture='crates/sepika-app/tests/fixtures/model.stb'
    fixture_sha256=(Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
    target=[ordered]@{source_type='StbGirder'; source_id='43'; name=$target[0].name; baseline_section='10'}
    sections=@($sections); endpoint_nodes=@($nodes)
    candidate_a_section='11'; candidate_b_section='23'; displacement_node='14'
    displacement_global_direction='Z'; unit='mm'
    design_load_target='saved combination: 保存長期架構G+P'; term='long'
    product_calculation='not executed in issue973'
}
$json = $manifest | ConvertTo-Json -Depth 8
if ($OutputPath) { Set-Content -LiteralPath $OutputPath -Value $json -Encoding utf8 }
$json
