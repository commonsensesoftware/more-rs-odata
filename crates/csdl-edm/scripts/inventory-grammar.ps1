$ErrorActionPreference = 'Stop'
$crateRoot = Split-Path $PSScriptRoot -Parent
$sourceDir = Join-Path $crateRoot 'data/conformance/oasis-4.01'
$files = @{
    'edm.xsd' = 'https://docs.oasis-open.org/odata/odata-csdl-xml/v4.01/os/schemas/edm.xsd'
    'edmx.xsd' = 'https://docs.oasis-open.org/odata/odata-csdl-xml/v4.01/os/schemas/edmx.xsd'
    'csdl.schema.json' = 'https://docs.oasis-open.org/odata/odata-csdl-json/v4.01/os/schemas/csdl.schema.json'
    'odata-abnf-construction-rules.txt' = 'https://docs.oasis-open.org/odata/odata/v4.01/os/abnf/odata-abnf-construction-rules.txt'
    'odata-abnf-testcases.xml' = 'https://docs.oasis-open.org/odata/odata/v4.01/os/abnf/odata-abnf-testcases.xml'
}
$sources = foreach ($name in ($files.Keys | Sort-Object)) {
    [ordered]@{ file=$name; source=$files[$name]; sha256=(Get-FileHash (Join-Path $sourceDir $name) -Algorithm SHA256).Hash.ToLowerInvariant() }
}
$families = @()
foreach ($name in @('edm.xsd','edmx.xsd')) {
    [xml]$schema = Get-Content (Join-Path $sourceDir $name) -Raw
    foreach ($node in $schema.DocumentElement.ChildNodes) {
        if ($node -is [System.Xml.XmlElement] -and $node.HasAttribute('name')) {
            $families += [ordered]@{ source=$name; family="$($node.LocalName):$($node.GetAttribute('name'))"; disposition='partial'; responsibility='CSDL parser shape and lexical rules; semantic rules also require resolver/validator checks. Family inventory is not exhaustive branch evidence.' }
        }
    }
}
$jsonSchema = Get-Content (Join-Path $sourceDir 'csdl.schema.json') -Raw | ConvertFrom-Json
foreach ($name in ($jsonSchema.definitions.PSObject.Properties.Name | Sort-Object)) {
    $families += [ordered]@{ source='csdl.schema.json'; family=$name; disposition='partial'; responsibility='Strict JSON source checks and shared CSDL grammar. Independent positive/negative branch evidence remains to be mapped.' }
}
$types = @{
    booleanValue='Edm.Boolean'; byteValue='Edm.Byte'; sbyteValue='Edm.SByte'; int16Value='Edm.Int16'; int32Value='Edm.Int32'; int64Value='Edm.Int64'
    dateValue='Edm.Date'; dateTimeOffsetValue='Edm.DateTimeOffset'; timeOfDayValue='Edm.TimeOfDay'; durationValue='Edm.Duration'
    decimalValue='Edm.Decimal'; doubleValue='Edm.Double'; singleValue='Edm.Single'; guidValue='Edm.Guid'; odataIdentifier='identifier'
}
$csdlPaths = @('propertyPathExpr','keyPathLiteral','keyPropertyAlias','primitiveLiteral','primitiveValue','enumMemberValue','enumerationMember','enumerationTypeName','namespacePart','termName','typeDefinitionName','complexTypeName','entityTypeName','identifierCharacter','identifierLeadingCharacter')
[xml]$suite = Get-Content (Join-Path $sourceDir 'odata-abnf-testcases.xml') -Raw
$cases = $suite.SelectNodes('//*[local-name()="TestCase"]')
foreach ($group in ($cases | Group-Object Rule | Sort-Object Name)) {
    $rule = $group.Name
    $disposition = if ($types.ContainsKey($rule)) { 'executed' } elseif ($rule -in $csdlPaths) { 'partial' } else { 'outside-static-scope' }
    $responsibility = switch ($disposition) {
        'executed' { 'tests/imported_grammar.rs: original positive/negative inputs, with explicit CSDL representation adaptation.' }
        'partial' { 'Related CSDL lexical/path family; URL grammar requires adaptation before use as CSDL evidence.' }
        default { 'URL/query parser, HTTP protocol, runtime values, or ABNF helper production; not a standalone CSDL declaration rule.' }
    }
    $families += [ordered]@{ source='odata-abnf-testcases.xml'; family=$rule; cases=$group.Count; disposition=$disposition; responsibility=$responsibility }
}
$selected = @()
for ($index=0; $index -lt $cases.Count; $index++) {
    $case=$cases[$index]
    if (-not $types.ContainsKey($case.Rule)) { continue }
    $valid = -not $case.HasAttribute('FailAt')
    $adaptation = $null
    if ($case.Rule -eq 'booleanValue' -and $case.Input -cnotin @('true','false')) {
        $valid = $false
        $adaptation = 'CSDL XML Bool uses xs:boolean and JSON Boolean uses lowercase JSON literals; URL case-insensitive tokens are not payload Booleans.'
    }
    $selected += [ordered]@{ index=$index; rule=$case.Rule; name=$case.Name; input=[string]$case.Input; type=$types[$case.Rule]; valid=$valid; adaptation=$adaptation }
}
[ordered]@{ sources=@($sources); families=$families; abnf_total_cases=$cases.Count; executable_cases=$selected } |
    ConvertTo-Json -Depth 12 | Set-Content (Join-Path $crateRoot 'docs/grammar-evidence.json') -Encoding utf8
