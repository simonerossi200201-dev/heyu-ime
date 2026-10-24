<#
.SYNOPSIS
  Build a clean, redistributable HEYU release (source tree + Windows runtime zip) and optionally a
  fresh orphan git branch 'public' that contains ONLY distributable files.

.DESCRIPTION
  * Every tracked file (git ls-files) must be classified in tools/release_manifest.json as
    restricted / personal / distributable. Unclassified files => FAIL.
  * Restricted (Xiaohe/openfly-derived tables, root-table helpers, generated manual) and personal files
    (user_custom.tsv, config.json, telemetry, logs, machine markers) are never copied.
  * After staging, independent safety checks run and FAIL LOUDLY on any hit:
      - hard-coded denylist of file names, anywhere in the tree
      - SHA-256 equality with any local restricted table (data/, %APPDATA%\HEYU\data[\private])
      - data files mentioning openfly / flypy / Xiaohe (in Chinese) / He Haifeng (in Chinese)
      - any *.tsv table with > 3000 rows outside data/open/
      - LICENSE, THIRD_PARTY_NOTICES.md, docs/LICENSING.md must be present
  * The personal phrase table is replaced by data/examples/user_custom.example.tsv.

.PARAMETER OutDir        Output directory (default: dist)
.PARAMETER SkipBuild     Do not run cargo build --release (use the existing target\release\heyu-ime.exe)
.PARAMETER SourceOnly    Only produce the source tree / branch (no runtime zip, no exe needed)
.PARAMETER PublicBranch  Also (re)create the orphan branch 'public' from the source tree
.PARAMETER Force         Replace an existing 'public' branch / output folder

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File tools\make_release.ps1 -SourceOnly -PublicBranch
#>
param(
    [string]$OutDir = "dist",
    [switch]$SkipBuild,
    [switch]$SourceOnly,
    [switch]$PublicBranch,
    [string]$BranchName = "public",
    [switch]$Force
)

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$Repo = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location -LiteralPath $Repo

function Fail([string]$msg) {
    Write-Host ""
    Write-Host "RELEASE BLOCKED: $msg" -ForegroundColor Red
    exit 1
}

function Matches-Any([string]$path, $entries) {
    foreach ($e in $entries) {
        $pat = if ($e -is [string]) { $e } else { $e.pattern }
        if ($path -like $pat) { return $e }
    }
    return $null
}

# ------------------------------------------------------------------------------------------------
$manifestPath = Join-Path $Repo "tools\release_manifest.json"
if (!(Test-Path -LiteralPath $manifestPath)) { Fail "missing tools/release_manifest.json" }
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json

# Hard-coded denylist (leaf names / regexes) - independent of the manifest, cannot be overridden.
$DenyLeaf = @("heyu_sys.tsv", "heyu_reverse.tsv", "shuangpin_shape_hints.tsv", "flypy_sys.tsv",
              "flypy_reverse.tsv", "root_atlas.json", "heyu_facts.py", "gen_manual.py",
              "audit_manual.py", "telemetry.json", "crash.log", "studio.log", "config.json",
              "security_audit_report.json", "tsf_registered.marker", "heyu_installed.marker")
$DenyRegex = @('(^|/)data/private/', '(^|/)tools/raw_sources/', '\u6307\u5357.*\.md$',    # *zhinan*.md (manual)
               '(?i)openfly', '(?i)flypy', '(?i)rime-flypy')
$ContentRegex = '(?i)openfly|flypy|\u5c0f\u9e64|\u4f55\u6d77\u5cf0'   # openfly|flypy|xiaohe|he-haifeng (CJK escapes)

$version = "0.0.0"
$m = Select-String -LiteralPath (Join-Path $Repo "Cargo.toml") -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
if ($m) { $version = $m.Matches[0].Groups[1].Value }

# ------------------------------------------------------------------------------------------------
Write-Host "[1/6] Classifying tracked files against tools/release_manifest.json ..." -ForegroundColor Cyan
$tracked = @(git -c core.quotepath=off ls-files) | Where-Object { $_ -ne "" }
if ($LASTEXITCODE -ne 0 -or $tracked.Count -eq 0) { Fail "git ls-files failed" }

$include = New-Object System.Collections.Generic.List[string]
$unclassified = @()
$skipped = @()
foreach ($f in $tracked) {
    if (!(Test-Path -LiteralPath (Join-Path $Repo $f))) { continue }   # deleted in working tree
    $r = Matches-Any $f $manifest.restricted
    if ($r) { $skipped += "restricted  $f"; continue }
    $p = Matches-Any $f $manifest.personal
    if ($p) { $skipped += "personal    $f"; continue }
    $d = Matches-Any $f $manifest.distributable
    if ($d) { $include.Add($f); continue }
    $unclassified += $f
}
if ($unclassified.Count -gt 0) {
    $unclassified | ForEach-Object { Write-Host "  unclassified: $_" -ForegroundColor Yellow }
    Fail "$($unclassified.Count) tracked file(s) are not classified in tools/release_manifest.json (add them as distributable / personal / restricted after checking their provenance)."
}
$skipped | ForEach-Object { Write-Host "  skip $_" -ForegroundColor DarkGray }
foreach ($req in @("LICENSE", "THIRD_PARTY_NOTICES.md", "docs/LICENSING.md", "data/open/heyu_open_sys.tsv",
                   "data/examples/user_custom.example.tsv")) {
    if (-not ($include -contains $req)) { Fail "required file '$req' is not tracked/committed" }
}

# ------------------------------------------------------------------------------------------------
Write-Host "[2/6] Staging source tree ..." -ForegroundColor Cyan
$outRoot = Join-Path $Repo $OutDir
$srcDir = Join-Path $outRoot "HEYU-$version-src"
$rtDir = Join-Path $outRoot "HEYU-$version-win64"
foreach ($d in @($srcDir, $rtDir)) {
    if (Test-Path -LiteralPath $d) {
        if (-not $Force) { Fail "$d already exists (use -Force)" }
        Remove-Item -Recurse -Force -LiteralPath $d
    }
}
New-Item -ItemType Directory -Force -Path $srcDir | Out-Null
foreach ($f in $include) {
    $dst = Join-Path $srcDir $f
    New-Item -ItemType Directory -Force -Path (Split-Path $dst -Parent) | Out-Null
    # use the committed blob, not the working copy, so uncommitted local edits never leak
    $tmp = [IO.Path]::GetTempFileName()
    cmd /c "git show `"HEAD:$f`" > `"$tmp`"" 2>$null
    if ($LASTEXITCODE -ne 0) { Remove-Item $tmp -Force; Fail "'$f' is staged but not committed - commit it first" }
    Move-Item -Force -LiteralPath $tmp -Destination $dst
}
Copy-Item -LiteralPath (Join-Path $srcDir "data\examples\user_custom.example.tsv") -Destination (Join-Path $srcDir "data\user_custom.tsv")

# ------------------------------------------------------------------------------------------------
function Test-ReleaseTree([string]$root) {
    Write-Host "      checking $root" -ForegroundColor DarkGray
    $restrictedHashes = @{}
    $cands = @()
    foreach ($n in @("heyu_sys.tsv", "heyu_reverse.tsv", "shuangpin_shape_hints.tsv", "flypy_sys.tsv", "flypy_reverse.tsv", "root_atlas.json")) {
        $cands += (Join-Path $Repo "data\$n")
        if ($env:APPDATA) {
            $cands += (Join-Path $env:APPDATA "HEYU\data\$n")
            $cands += (Join-Path $env:APPDATA "HEYU\data\private\$n")
        }
    }
    foreach ($c in $cands) {
        if (Test-Path -LiteralPath $c) { $restrictedHashes[(Get-FileHash -LiteralPath $c -Algorithm SHA256).Hash] = $c }
    }
    $rootFull = (Resolve-Path -LiteralPath $root).Path
    Get-ChildItem -LiteralPath $root -Recurse -File | ForEach-Object {
        $rel = $_.FullName.Substring($rootFull.Length + 1).Replace('\', '/')
        if ($DenyLeaf -contains $_.Name) { Fail "denylisted file in release: $rel" }
        foreach ($rx in $DenyRegex) { if ($rel -match $rx) { Fail "denylisted path in release: $rel (rule $rx)" } }
        if (Matches-Any $rel $manifest.restricted) { Fail "restricted file in release: $rel" }
        $h = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
        if ($restrictedHashes.ContainsKey($h)) { Fail "$rel is byte-identical to restricted $($restrictedHashes[$h])" }
        if ($rel -like "data/*" -and $_.Extension -in @(".tsv", ".json", ".txt", ".md")) {
            $hit = Select-String -LiteralPath $_.FullName -Pattern $ContentRegex -Encoding UTF8 | Select-Object -First 1
            if ($hit) { Fail "$rel line $($hit.LineNumber) mentions a restricted source: $($hit.Line.Trim())" }
        }
        if ($_.Extension -eq ".tsv" -and $rel -notlike "data/open/*") {
            $rows = (Get-Content -LiteralPath $_.FullName -Encoding UTF8 | Where-Object { $_ -and -not $_.StartsWith("#") }).Count
            if ($rows -gt 3000) { Fail "$rel has $rows rows - large tables must live in data/open/ with documented provenance" }
        }
    }
}

Write-Host "[3/6] Safety checks on the source tree ..." -ForegroundColor Cyan
Test-ReleaseTree $srcDir

# ------------------------------------------------------------------------------------------------
if (-not $SourceOnly) {
    Write-Host "[4/6] Building runtime package ..." -ForegroundColor Cyan
    if (-not $SkipBuild) {
        cargo build --release
        if ($LASTEXITCODE -ne 0) { Fail "cargo build --release failed" }
    }
    $exe = Join-Path $Repo "target\release\heyu-ime.exe"
    $voiceExe = Join-Path $Repo "target\release\heyu-voice.exe"
    if (!(Test-Path -LiteralPath $exe)) { Fail "missing $exe (build first or use -SourceOnly)" }
    New-Item -ItemType Directory -Force -Path $rtDir | Out-Null
    Copy-Item -LiteralPath $exe -Destination (Join-Path $rtDir "heyu-ime.exe")
    if (Test-Path -LiteralPath $voiceExe) {
        Copy-Item -LiteralPath $voiceExe -Destination (Join-Path $rtDir "heyu-voice.exe")
    }
    $srcFull = (Resolve-Path -LiteralPath $srcDir).Path
    Get-ChildItem -LiteralPath $srcDir -Recurse -File | ForEach-Object {
        $rel = $_.FullName.Substring($srcFull.Length + 1).Replace('\', '/')
        if (Matches-Any $rel $manifest.runtime) {
            $dst = Join-Path $rtDir $rel
            New-Item -ItemType Directory -Force -Path (Split-Path $dst -Parent) | Out-Null
            Copy-Item -LiteralPath $_.FullName -Destination $dst
        }
    }
    Test-ReleaseTree $rtDir
    $zip = Join-Path $outRoot "HEYU-$version-win64.zip"
    if (Test-Path -LiteralPath $zip) { Remove-Item -Force -LiteralPath $zip }
    Compress-Archive -Path (Join-Path $rtDir "*") -DestinationPath $zip
    Write-Host "      -> $zip" -ForegroundColor Green

    # Also produce a 100% self-contained Portable edition zip with portable.flag pre-enabled
    $portFile = Join-Path $rtDir "portable.flag"
    Set-Content -LiteralPath $portFile -Value "HEYU Portable Mode (all config & encrypted vault stay in ./data)" -Encoding UTF8
    $portZip = Join-Path $outRoot "HEYU-$version-portable-win64.zip"
    if (Test-Path -LiteralPath $portZip) { Remove-Item -Force -LiteralPath $portZip }
    Compress-Archive -Path (Join-Path $rtDir "*") -DestinationPath $portZip
    Write-Host "      -> $portZip (Portable Edition)" -ForegroundColor Green
} else {
    Write-Host "[4/6] Runtime package skipped (-SourceOnly)" -ForegroundColor DarkGray
}

$srcZip = Join-Path $outRoot "HEYU-$version-src.zip"
if (Test-Path -LiteralPath $srcZip) { Remove-Item -Force -LiteralPath $srcZip }
Compress-Archive -Path (Join-Path $srcDir "*") -DestinationPath $srcZip
Write-Host "[5/6] Source archive -> $srcZip" -ForegroundColor Green

# ------------------------------------------------------------------------------------------------
if ($PublicBranch) {
    Write-Host "[6/6] Creating orphan branch '$BranchName' (history-free) ..." -ForegroundColor Cyan
    git show-ref --verify --quiet "refs/heads/$BranchName"
    if ($LASTEXITCODE -eq 0) {
        if (-not $Force) { Fail "branch '$BranchName' already exists (use -Force to recreate it)" }
        git branch -D $BranchName | Out-Null
    }
    $wt = Join-Path ([IO.Path]::GetTempPath()) ("heyu_public_" + [Guid]::NewGuid().ToString("N").Substring(0, 8))
    git worktree add --detach "$wt" HEAD | Out-Null
    if ($LASTEXITCODE -ne 0) { Fail "git worktree add failed" }
    try {
        Push-Location -LiteralPath $wt
        git checkout --quiet --orphan $BranchName
        git rm -r -f --quiet --cached . | Out-Null
        Get-ChildItem -LiteralPath $wt -Force | Where-Object { $_.Name -ne ".git" } | Remove-Item -Recurse -Force
        Copy-Item -Path (Join-Path $srcDir "*") -Destination $wt -Recurse -Force
        Get-ChildItem -LiteralPath $srcDir -Force -Filter ".*" | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $wt -Recurse -Force }
        Test-ReleaseTree $wt   # never commit anything that fails the checks
        git add --all -- . 
        git commit --quiet -m "HEYU $version - public release (distributable files only, see docs/LICENSING.md)"
        if ($LASTEXITCODE -ne 0) { Fail "commit on '$BranchName' failed" }
        $count = @(git ls-files).Count
        Write-Host "      branch '$BranchName' created with $count files and no prior history" -ForegroundColor Green
    } finally {
        Pop-Location
        git worktree remove --force "$wt" | Out-Null
    }
    Write-Host "      publish with:  git push <github-remote> ${BranchName}:main" -ForegroundColor Green
} else {
    Write-Host "[6/6] Orphan branch skipped (use -PublicBranch)" -ForegroundColor DarkGray
}
Write-Host "Release OK." -ForegroundColor Green
