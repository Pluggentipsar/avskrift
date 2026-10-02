param([string]$OutputDir = (Join-Path $PSScriptRoot '../.build-tools/pianissimo/fixtures'))
$ErrorActionPreference = 'Stop'
# Uses an installed Windows OneCore voice through SAPI; changes no registry keys.
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
$speaker = New-Object -ComObject SAPI.SpVoice
$category = New-Object -ComObject SAPI.SpObjectTokenCategory
$stream = $null
try {
    $category.SetId('HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Speech_OneCore\Voices', $false)
    $voice = @($category.EnumerateTokens() | Where-Object { $_.GetAttribute('Language') -match '(^|;)0?41d($|;)' })[0]
    if (!$voice) { throw 'Installera en svensk Windows-röst för de syntetiska testfallen.' }
    $speaker.Voice = $voice
    $cases = [ordered]@{
        'dictation' = 'Boka ett möte med Åsa Lindström på torsdag. Vi ska inte beställa någon ny skrivare innan felet är undersökt.'
        'numbers' = 'Ärendet gäller skrivare tjugotre på plan två. Ring Erik klockan fjorton trettio. Kostnaden är ett tusen två hundra kronor.'
        'negations' = 'Patienten har inte feber och tar inte penicillin. Uppgiften om allergi är inte bekräftad. Vi ska kontrollera den innan nästa besök.'
    }
    foreach ($case in $cases.GetEnumerator()) {
        $wav = [IO.Path]::GetFullPath((Join-Path $OutputDir ($case.Key + '.wav')))
        $stream = New-Object -ComObject SAPI.SpFileStream
        $stream.Format.Type = 18 # SAFT16kHz16BitMono
        $stream.Open($wav, 3, $false) # SSFMCreateForWrite
        $speaker.AudioOutputStream = $stream
        $speaker.Speak($case.Value) | Out-Null
        $stream.Close()
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($stream)
        $stream = $null
        [IO.File]::WriteAllText((Join-Path $OutputDir ($case.Key + '.txt')), $case.Value, (New-Object Text.UTF8Encoding($false)))
    }
    $demo = Join-Path $PSScriptRoot '../docs/demo-support'
    Copy-Item -LiteralPath (Join-Path $demo 'fiktivt-supportsamtal.wav') -Destination (Join-Path $OutputDir 'support.wav') -Force
    $source = Get-Content -LiteralPath (Join-Path $demo 'forberett-exempel.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    # The bundled recording reads the speaker labels aloud too.
    $reference = $source.lines -join ' '
    [IO.File]::WriteAllText((Join-Path $OutputDir 'support.txt'), $reference, (New-Object Text.UTF8Encoding($false)))
    @{ synthetic = $true; voice = $voice.GetDescription(); culture = 'sv-SE';
       note = 'Fiktiva testfall. Stödjer inte slutsatser om verkliga möten, dialekter eller överlappande tal.' } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $OutputDir 'fixtures.json') -Encoding UTF8
} finally {
    if ($stream) { $stream.Close(); [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($stream) }
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($speaker)
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($category)
}
Write-Host "Testljud: $OutputDir"
