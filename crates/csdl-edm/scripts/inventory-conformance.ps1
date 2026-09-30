param([string]$CacheDirectory = $env:TEMP)
$ErrorActionPreference = 'Stop'
$crateDirectory = Split-Path $PSScriptRoot -Parent
$rows = [System.Collections.Generic.List[object]]::new()
$sources = @()
foreach ($format in @('xml', 'json')) {
    $url = "https://docs.oasis-open.org/odata/odata-csdl-$format/v4.01/os/odata-csdl-$format-v4.01-os.html"
    $cache = Join-Path $CacheDirectory "csdl-$format-spec.html"
    if (-not (Test-Path -LiteralPath $cache)) {
        (Invoke-WebRequest $url).Content | Set-Content -LiteralPath $cache
    }
    $html = Get-Content -LiteralPath $cache -Raw
    if ($format -eq 'xml') {
        $notice = [System.Collections.Generic.List[string]]::new()
        $inNotice = $false
        foreach ($p in [regex]::Matches($html, '(?is)<p\b[^>]*>.*?</p>')) {
            $text = [regex]::Replace([System.Net.WebUtility]::HtmlDecode([regex]::Replace($p.Value, '<[^>]+>', '')), '\s+', ' ').Trim()
            if ($text.StartsWith('Copyright © OASIS Open')) { $inNotice = $true }
            if ($inNotice -and $text -eq 'Table of Contents') { break }
            if ($inNotice) { $notice.Add($text) }
        }
        if ($notice.Count -lt 5) { throw 'Could not extract the complete OASIS Notices section' }
        ("# OASIS specification notice`n`nSource: $url`n`n" + ($notice -join "`n`n")) |
            Set-Content -LiteralPath (Join-Path $crateDirectory 'docs/OASIS-NOTICE.md') -Encoding utf8NoBOM
    }
    $section = ''
    $anchor = ''
    $ordinal = 0
    $counts = @{}
    foreach ($block in [regex]::Matches($html, '(?is)<(?<tag>h[1-6]|p)\b[^>]*>(?<body>.*?)</\k<tag>>')) {
        $body = $block.Groups['body'].Value
        $text = [System.Net.WebUtility]::HtmlDecode([regex]::Replace($body, '<[^>]+>', ''))
        $text = [regex]::Replace($text, '\s+', ' ').Trim()
        if ($block.Groups['tag'].Value.StartsWith('h')) {
            $section = $text
            $link = [regex]::Match($body, 'href="#(?<anchor>[^"]+)"')
            if (-not $link.Success) { $link = [regex]::Match($body, 'name="(?<anchor>[^"]+)"') }
            $anchor = $link.Groups['anchor'].Value
            $ordinal = 0
            continue
        }
        if ($section -notmatch '^\d' -or $text -cnotmatch '\b(MUST|SHOULD|SHALL|REQUIRED|RECOMMENDED)\b') { continue }
        $ordinal++
        $number = [regex]::Match($section, '^\d+(?:\.\d+)*').Value
        $id = "$format-$number-$ordinal"
        if ($counts.ContainsKey($id)) { throw "Duplicate requirement ID $id" }
        $counts[$id] = $true
        $rows.Add([ordered]@{
            id = $id
            representation = $format
            section = $section
            source = "$url#$anchor"
            requirement = $text
            keywords = @([regex]::Matches($text, '\b(?:MUST NOT|SHOULD NOT|SHALL NOT|MUST|SHOULD|SHALL|REQUIRED|RECOMMENDED)\b') | ForEach-Object Value | Sort-Object -Unique)
        })
    }
    $sources += [ordered]@{ representation = $format; url = $url; requirement_paragraphs = $counts.Count }
}
$inventory = [ordered]@{
    specification = 'OASIS CSDL XML and JSON 4.01 OS, 11 May 2020'
    license = 'Copyright OASIS Open 2020. Specification Notices permit copying and derivative works that explain or assist implementation. See source Notices and docs/conformance-audit.md.'
    extraction = 'Normative paragraphs (a paragraph may contain several obligations); MUST, MUST NOT, SHOULD, SHOULD NOT, SHALL, REQUIRED, RECOMMENDED. MAY permissions are reviewed in the section-level audit.'
    sources = $sources
    requirements = $rows.ToArray()
}
$inventory | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $crateDirectory 'docs/conformance-requirements.json') -Encoding utf8NoBOM
Write-Output "Inventoried $($rows.Count) normative paragraphs."
