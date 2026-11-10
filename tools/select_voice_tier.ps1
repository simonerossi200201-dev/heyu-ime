# HEYU v2 voice: hardware tier selection (pure ASCII; Chinese text built from code points).
# Writes/merges voice_config.json:  NVIDIA GPU >= 6 GB  -> large-v3-turbo-q8_0 + live preview
#                                   otherwise            -> small-q5_1 (CPU), no live preview
# LLM polish always stays OFF by default (enable manually, see README).
param(
    [string]$DataDir = "",
    [switch]$DryRun
)
$ErrorActionPreference = "Stop"
function U([int[]]$c) { -join ($c | ForEach-Object { [char]$_ }) }

if (-not $DataDir) { $DataDir = Join-Path $env:APPDATA "HEYU\data" }
New-Item -ItemType Directory -Force -Path $DataDir | Out-Null

$vramMB = 0; $gpuName = ""
$smi = $null
foreach ($cand in @("nvidia-smi.exe", "$env:SystemRoot\System32\nvidia-smi.exe", "$env:ProgramFiles\NVIDIA Corporation\NVSMI\nvidia-smi.exe")) {
    $c = Get-Command $cand -ErrorAction SilentlyContinue
    if ($c) { $smi = $c.Source; break }
}
if ($smi) {
    try {
        $out = & $smi --query-gpu=name,memory.total --format=csv,noheader,nounits 2>$null | Select-Object -First 1
        if ($out) {
            $parts = $out.Split(',')
            $gpuName = $parts[0].Trim()
            $vramMB = [int]($parts[1].Trim())
        }
    } catch {}
}
$hasNvidia = $vramMB -gt 0
$tier = if ($hasNvidia -and $vramMB -ge 6000) { "gpu" } else { "cpu" }
$cores = (Get-CimInstance Win32_Processor | Measure-Object -Property NumberOfCores -Sum).Sum

$cfgPath = Join-Path $DataDir "voice_config.json"
$cfg = [ordered]@{}
if (Test-Path -LiteralPath $cfgPath) {
    try {
        $j = Get-Content -LiteralPath $cfgPath -Raw -Encoding UTF8 | ConvertFrom-Json
        foreach ($p in $j.PSObject.Properties) { $cfg[$p.Name] = $p.Value }
    } catch {}
}
$appRoot = Split-Path -Parent $PSScriptRoot
$modelDirs = @(
    (Join-Path $appRoot "models"),
    (Join-Path (Split-Path -Parent $DataDir) "models"),
    (Join-Path $env:APPDATA "HEYU\models")
)
function Find-InstalledModel([string[]]$candidates) {
    foreach ($m in $candidates) {
        foreach ($d in $modelDirs) {
            if ($d -and (Test-Path -LiteralPath (Join-Path $d "ggml-$m.bin"))) {
                return $m
            }
        }
    }
    return $candidates[0]
}

if ($tier -eq "gpu") {
    $chosenModel = Find-InstalledModel @("large-v3-turbo-q8_0", "large-v3-q5_0", "large-v3-turbo-q5_0", "small-q5_1", "base-q5_1")
    $cfg["model"] = $chosenModel; $cfg["use_gpu"] = "auto"; $cfg["live_preview"] = $true
} else {
    $chosenModel = Find-InstalledModel @("small-q5_1", "large-v3-turbo-q5_0", "base-q5_1", "large-v3-turbo-q8_0")
    $cfg["model"] = $chosenModel; $cfg["use_gpu"] = "false"; $cfg["live_preview"] = $false
}
$cfg["pipeline"] = "v2"
if (-not $cfg.Contains("llm_polish")) { $cfg["llm_polish"] = ($tier -eq "gpu") }
if (-not $cfg.Contains("llm_model"))  { $cfg["llm_model"] = "gemma4:12b" }
if (-not $cfg.Contains("hotkey"))     { $cfg["hotkey"] = "Ctrl+Alt+V" }
if (-not $cfg.Contains("continuous_mode")) { $cfg["continuous_mode"] = $true }
if (-not $cfg.Contains("simplified_chinese")) { $cfg["simplified_chinese"] = $true }

$json = ($cfg | ConvertTo-Json -Depth 3)
$summary = "tier=$tier gpu='$gpuName' vram_mb=$vramMB cpu_cores=$cores model=$($cfg['model'])"
if ($DryRun) { Write-Host $summary; Write-Host $json; exit 0 }
if (Test-Path -LiteralPath $cfgPath) { Copy-Item -LiteralPath $cfgPath -Destination "$cfgPath.bak_before_v2" -Force }
[IO.File]::WriteAllText($cfgPath, $json, (New-Object Text.UTF8Encoding $false))
$hw = Join-Path $DataDir "voice_hotwords.txt"
$hwSrc = Join-Path (Split-Path -Parent $PSScriptRoot) "data\voice_hotwords.txt"
if (-not (Test-Path -LiteralPath $hw) -and (Test-Path -LiteralPath $hwSrc)) { Copy-Item -LiteralPath $hwSrc -Destination $hw }
Set-Content -LiteralPath (Join-Path $DataDir "voice_tier.txt") -Value $summary -Encoding Ascii
if ($tier -eq "gpu") {
    Write-Host ((U @(0x5DF2,0x68C0,0x6D4B,0x5230)) + " NVIDIA $gpuName ($vramMB MB) -> large-v3-turbo-q8_0") -ForegroundColor Green
} else {
    Write-Host ((U @(0x672A,0x68C0,0x6D4B,0x5230,0x8DB3,0x591F,0x7684)) + " NVIDIA GPU -> CPU small-q5_1") -ForegroundColor Yellow
}