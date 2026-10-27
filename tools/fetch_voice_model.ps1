<#
.SYNOPSIS
    鹤羽音形 HEYU · 语音输入 — 一次性下载离线语音模型 / 推理运行库（带固定 SHA-256 校验）

.DESCRIPTION
    这是整个 HEYU 语音输入中【唯一】会联网的组件，只在你手动运行时才联网一次。
    输入法本体 heyu-ime.exe、语音进程 heyu-voice.exe 以及 whisper 推理库都不含任何网络 API，
    并且 install.ps1 会为它们添加 Windows 防火墙“出站阻止”规则。

    下载来源（均为美国公司托管，不经过中国大陆服务器）：
      * 模型权重：Hugging Face  huggingface.co/ggerganov/whisper.cpp （固定到提交 $HF_REVISION）
      * 推理运行库：GitHub Releases  github.com/ggml-org/whisper.cpp  （固定到 $RT_TAG）
    每个文件都用下方写死的 SHA-256 校验；任何一个字节不对都会拒绝安装并以非零退出码结束。

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tools\fetch_voice_model.ps1
        # 默认：下载 large-v3-turbo-q5_0 模型 + 推理运行库（有 NVIDIA 显卡时自动选 CUDA 版）
.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tools\fetch_voice_model.ps1 -Model small-q5_1
.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tools\fetch_voice_model.ps1 -Runtime -Variant cpu
.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tools\fetch_voice_model.ps1 -List
#>
param(
    [ValidateSet('large-v3', 'large-v3-q5_0', 'large-v3-turbo-q5_0', 'medium-q5_0', 'small-q5_1', 'base-q5_1')]
    [string]$Model = '',
    [switch]$Runtime,
    [ValidateSet('auto', 'cpu', 'cuda')]
    [string]$Variant = 'auto',
    [string]$ModelsDir = '',
    [string]$RuntimeDir = '',
    [switch]$List
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'Continue'
try { [Console]::OutputEncoding = [Text.Encoding]::UTF8 } catch {}

# ---------------------------------------------------------------------------------------------
# Pinned sources. Hashes come from the Hugging Face LFS metadata (lfs.oid = SHA-256 of the file)
# and from the GitHub release asset digests; all were re-verified by downloading + hashing.
# ---------------------------------------------------------------------------------------------
$HF_REVISION = '5359861c739e955e79d9a303bcbc70fb988958b1'
$MODELS = [ordered]@{
    'large-v3-q5_0'       = @{ Size = 1081140203; Sha = 'd75795ecff3f83b5faa89d1900604ad8c780abd5739fae406de19f23ecd98ad1'; Desc = '旗舰满血 32层解码器：中文同音字/多语种最高语义精度（约 1.03 GB，独显强烈推荐）' }
    'large-v3'            = @{ Size = 3095033483; Sha = '64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2'; Desc = '旗舰无损 FP16 满血版：32层编码+32层解码无量化极限精度（约 2.88 GB，16G显存尊享）' }
    'large-v3-turbo-q5_0' = @{ Size = 574041195;  Sha = '394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2'; Desc = '极速蒸馏版（4层解码器）：速度最快，适合普通配置（约 548 MB）' }
    'medium-q5_0'         = @{ Size = 539212467;  Sha = '19fea4b380c3a618ec4723c3eef2eb785ffba0d0538cf43f8f235e7b3b34220f'; Desc = '老一代 medium，速度较慢（约 514 MB）' }
    'small-q5_1'          = @{ Size = 190085487;  Sha = 'ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb'; Desc = '轻量：无独显的旧电脑（约 181 MB）' }
    'base-q5_1'           = @{ Size = 59707625;   Sha = '422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898'; Desc = '极小：仅用于测试（约 57 MB）' }
}

$RT_TAG = 'b5454'   # = whisper.cpp v1.9.5 (2026-10-06)
$RUNTIMES = [ordered]@{
    'cpu'  = @{ File = 'whisper-bin-x64.zip';                 Size = 8928640;   Sha = '6ba69e3482d7826214f90a6a9c84ca07782aec1e1d0c6a7c30c994fd5d816ccb'; Desc = 'CPU 版（约 9 MB）' }
    'cuda' = @{ File = 'whisper-bin-win-cuda-12.4.0-x64.zip'; Size = 684913404; Sha = 'afef0b881c500958921c3f5523b50e59ee2ec9b6f5cbd25b324c51ed308a957a'; Desc = 'NVIDIA CUDA 12.4 版（约 653 MB，含 cuBLAS）' }
}
# Only these files are kept from the release zip. Everything else (whisper-server.exe, which
# links ws2_32 for HTTP, SDL2 tools, llama.dll, tests ...) is discarded.
$RT_KEEP = @('whisper-cli.exe', 'whisper.dll', 'ggml.dll', 'ggml-base.dll', 'ggml-cpu-*.dll', 'ggml-cuda.dll',
             'cublas64_*.dll', 'cublasLt64_*.dll', 'cudart64_*.dll')
$FORBIDDEN_IMPORTS = @('ws2_32.dll', 'wsock32.dll', 'wininet.dll', 'winhttp.dll', 'dnsapi.dll', 'mswsock.dll',
                       'iphlpapi.dll', 'urlmon.dll', 'cryptnet.dll', 'webio.dll', 'httpapi.dll')

$RepoRoot = Split-Path -Parent $PSScriptRoot
if (-not $ModelsDir) { $ModelsDir = Join-Path $env:APPDATA 'HEYU\models' }

function Test-DirWritable([string]$dir) {
    try {
        New-Item -ItemType Directory -Force -Path $dir -ErrorAction Stop | Out-Null
        $probe = Join-Path $dir ('.write_probe_' + [Guid]::NewGuid().ToString('N'))
        Set-Content -LiteralPath $probe -Value 'x' -ErrorAction Stop
        Remove-Item -LiteralPath $probe -Force
        return $true
    } catch { return $false }
}
if (-not $RuntimeDir) {
    $RuntimeDir = Join-Path $RepoRoot 'voice-runtime'
    if (-not (Test-DirWritable $RuntimeDir)) { $RuntimeDir = Join-Path $env:LOCALAPPDATA 'HEYU\voice-runtime' }
}

function Say([string]$msg, [string]$color = 'Cyan') { Write-Host $msg -ForegroundColor $color }

function Get-Sha256([string]$path) { (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }

function Format-MB([long]$b) { '{0:N1} MB' -f ($b / 1MB) }

# Resumable download: <dest>.part grows across runs; on completion it is hashed and only then renamed.
function Get-Verified([string]$url, [string]$dest, [long]$size, [string]$sha, [string]$label) {
    if ((Test-Path -LiteralPath $dest) -and ((Get-Item -LiteralPath $dest).Length -eq $size)) {
        Say "  · 已存在，正在校验 SHA-256：$label"
        if ((Get-Sha256 $dest) -eq $sha) { Say "  ✔ 校验通过（已安装）" 'Green'; return }
        Say "  ✘ 现有文件校验失败，重新下载" 'Yellow'
        Remove-Item -LiteralPath $dest -Force
    }
    $part = "$dest.part"
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    for ($attempt = 1; $attempt -le 5; $attempt++) {
        $have = 0L
        if (Test-Path -LiteralPath $part) { $have = (Get-Item -LiteralPath $part).Length }
        if ($have -gt $size) { Remove-Item -LiteralPath $part -Force; $have = 0L }
        if ($have -eq $size) { break }
        try {
            $req = [Net.HttpWebRequest]::Create($url)
            $req.UserAgent = 'HEYU-voice-fetch/1.0'
            $req.AllowAutoRedirect = $true
            $req.Timeout = 30000
            $req.ReadWriteTimeout = 60000
            if ($have -gt 0) { $req.AddRange($have) ; Say "  · 断点续传：从 $(Format-MB $have) 继续" }
            $resp = $req.GetResponse()
            if ($have -gt 0 -and [int]$resp.StatusCode -ne 206) {
                # Server ignored the range request: start over.
                $have = 0L
                $fs = [IO.File]::Open($part, [IO.FileMode]::Create, [IO.FileAccess]::Write)
            } else {
                $fs = [IO.File]::Open($part, [IO.FileMode]::Append, [IO.FileAccess]::Write)
            }
            $in = $resp.GetResponseStream()
            $buf = New-Object byte[] (1MB)
            $done = $have
            $sw = [Diagnostics.Stopwatch]::StartNew()
            $lastTick = 0
            try {
                while (($n = $in.Read($buf, 0, $buf.Length)) -gt 0) {
                    $fs.Write($buf, 0, $n)
                    $done += $n
                    if ($sw.ElapsedMilliseconds - $lastTick -ge 400) {
                        $lastTick = $sw.ElapsedMilliseconds
                        $speed = ($done - $have) / [Math]::Max(0.001, $sw.Elapsed.TotalSeconds)
                        $pct = [int](100 * $done / $size)
                        Write-Progress -Activity "下载 $label" -Status ("{0} / {1}  ·  {2}/s" -f (Format-MB $done), (Format-MB $size), (Format-MB $speed)) -PercentComplete ([Math]::Min(100, $pct))
                    }
                }
            } finally {
                $fs.Close(); $in.Close(); $resp.Close()
                Write-Progress -Activity "下载 $label" -Completed
            }
        } catch {
            Say "  ! 网络错误（第 $attempt/5 次）：$($_.Exception.Message)" 'Yellow'
            Start-Sleep -Seconds ([Math]::Min(20, 3 * $attempt))
        }
    }
    if (!(Test-Path -LiteralPath $part) -or (Get-Item -LiteralPath $part).Length -ne $size) {
        Say "  ✘ 下载未完成，请检查网络后重新运行本脚本（已下载部分会保留并续传）。" 'Red'
        exit 2
    }
    Say "  · 下载完成，正在校验 SHA-256 ..."
    $got = Get-Sha256 $part
    if ($got -ne $sha) {
        Remove-Item -LiteralPath $part -Force
        Say "  ✘ SHA-256 不匹配！文件可能被篡改或损坏，已删除。" 'Red'
        Say "    期望：$sha" 'Red'
        Say "    实际：$got" 'Red'
        exit 3
    }
    Move-Item -Force -LiteralPath $part -Destination $dest
    Say "  ✔ SHA-256 校验通过：$sha" 'Green'
}

# Minimal PE import-table reader (same logic as tools/security_audit.py).
function Get-PeImports([string]$path) {
    $d = [IO.File]::ReadAllBytes($path)
    if ($d[0] -ne 0x4D -or $d[1] -ne 0x5A) { return @() }
    $pe = [BitConverter]::ToInt32($d, 0x3C)
    $nsec = [BitConverter]::ToUInt16($d, $pe + 6)
    $optSize = [BitConverter]::ToUInt16($d, $pe + 20)
    $opt = $pe + 24
    $magic = [BitConverter]::ToUInt16($d, $opt)
    $dd = $opt + $(if ($magic -eq 0x20B) { 112 } else { 96 })
    $impRva = [BitConverter]::ToUInt32($d, $dd + 8)
    if ($impRva -eq 0) { return @() }
    $secs = @()
    for ($i = 0; $i -lt $nsec; $i++) {
        $so = $opt + $optSize + $i * 40
        $secs += , @([BitConverter]::ToUInt32($d, $so + 12), [Math]::Max([BitConverter]::ToUInt32($d, $so + 8), [BitConverter]::ToUInt32($d, $so + 16)), [BitConverter]::ToUInt32($d, $so + 20))
    }
    function RvaOff($rva) { foreach ($s in $secs) { if ($rva -ge $s[0] -and $rva -lt ($s[0] + $s[1])) { return $s[2] + ($rva - $s[0]) } }; return -1 }
    $pos = RvaOff $impRva
    $names = @()
    while ($pos -ge 0 -and $pos + 20 -le $d.Length) {
        $oft = [BitConverter]::ToUInt32($d, $pos); $nameRva = [BitConverter]::ToUInt32($d, $pos + 12); $ft = [BitConverter]::ToUInt32($d, $pos + 16)
        if ($oft -eq 0 -and $nameRva -eq 0 -and $ft -eq 0) { break }
        $no = RvaOff $nameRva
        if ($no -ge 0) { $e = [Array]::IndexOf($d, [byte]0, [int]$no); $names += [Text.Encoding]::ASCII.GetString($d, $no, $e - $no) }
        $pos += 20
    }
    return $names
}

function Show-Status {
    Say "模型目录：$ModelsDir"
    foreach ($k in $MODELS.Keys) {
        $p = Join-Path $ModelsDir "ggml-$k.bin"
        $state = if (Test-Path -LiteralPath $p) { if ((Get-Item -LiteralPath $p).Length -eq $MODELS[$k].Size) { '已安装' } else { '大小不符' } } else { '未安装' }
        Say ("  {0,-22} {1,-6} {2}" -f $k, $state, $MODELS[$k].Desc) 'Gray'
    }
    $cli = Join-Path $RuntimeDir 'whisper-cli.exe'
    $kind = if (Test-Path -LiteralPath (Join-Path $RuntimeDir 'ggml-cuda.dll')) { 'CUDA' } elseif (Test-Path -LiteralPath $cli) { 'CPU' } else { '未安装' }
    Say "运行库目录：$RuntimeDir   [$kind]"
}

function Install-Model([string]$name) {
    $m = $MODELS[$name]
    Say ""
    Say "【语音模型】ggml-$name.bin — $($m.Desc)"
    New-Item -ItemType Directory -Force -Path $ModelsDir | Out-Null
    $url = "https://huggingface.co/ggerganov/whisper.cpp/resolve/$HF_REVISION/ggml-$name.bin"
    Get-Verified $url (Join-Path $ModelsDir "ggml-$name.bin") $m.Size $m.Sha "ggml-$name.bin"
}

function Install-Runtime([string]$variant) {
    if ($variant -eq 'auto') {
        $variant = 'cpu'
        $smi = Get-Command nvidia-smi.exe -ErrorAction SilentlyContinue
        if ($smi) {
            try { $gpu = & $smi.Source --query-gpu=name --format=csv,noheader 2>$null | Select-Object -First 1 } catch { $gpu = $null }
            if ($gpu) { $variant = 'cuda'; Say "检测到 NVIDIA 显卡：$gpu → 使用 CUDA 加速版" }
        }
    }
    $r = $RUNTIMES[$variant]
    Say ""
    Say "【推理运行库】whisper.cpp $RT_TAG · $($r.Desc)"
    $cache = Join-Path $env:LOCALAPPDATA 'HEYU\downloads'
    New-Item -ItemType Directory -Force -Path $cache | Out-Null
    $zip = Join-Path $cache "$RT_TAG-$($r.File)"
    $url = "https://github.com/ggml-org/whisper.cpp/releases/download/$RT_TAG/$($r.File)"
    Get-Verified $url $zip $r.Size $r.Sha $r.File

    $tmp = Join-Path $cache ("extract_" + [Guid]::NewGuid().ToString('N'))
    Say "  · 解压并只保留必要文件 ..."
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $za = [IO.Compression.ZipFile]::OpenRead($zip)
    try {
        foreach ($entry in $za.Entries) {
            if ($entry.FullName -match '(^|[\\/])\.\.([\\/]|$)' -or $entry.FullName -match ':') {
                throw "Unsafe zip entry path detected (Zip-Slip protection): $($entry.FullName)"
            }
        }
    } finally {
        $za.Dispose()
    }
    [IO.Compression.ZipFile]::ExtractToDirectory($zip, $tmp)
    $files = Get-ChildItem -LiteralPath $tmp -Recurse -File | Where-Object { $n = $_.Name; ($RT_KEEP | Where-Object { $n -like $_ }).Count -gt 0 }
    if (-not ($files | Where-Object { $_.Name -eq 'whisper.dll' })) {
        Remove-Item -Recurse -Force -LiteralPath $tmp
        Say "  ✘ 压缩包中没有 whisper.dll，结构与预期不符。" 'Red'; exit 4
    }
    # Network-capability audit BEFORE anything is installed.
    $bad = @()
    foreach ($f in $files) {
        $imps = Get-PeImports $f.FullName | ForEach-Object { $_.ToLowerInvariant() }
        $hit = $imps | Where-Object { $FORBIDDEN_IMPORTS -contains $_ }
        if ($hit) { $bad += "$($f.Name): $($hit -join ', ')" }
    }
    if ($bad.Count -gt 0) {
        Remove-Item -Recurse -Force -LiteralPath $tmp
        Say "  ✘ 安全审计失败：以下文件导入了网络库，拒绝安装：" 'Red'
        $bad | ForEach-Object { Say "    $_" 'Red' }
        exit 5
    }
    Say "  ✔ 安全审计通过：$($files.Count) 个文件均未导入任何网络库（ws2_32/winhttp/wininet/...）" 'Green'

    Get-Process -Name 'heyu-voice' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $RuntimeDir) {
        Get-ChildItem -LiteralPath $RuntimeDir -File | Remove-Item -Force -ErrorAction SilentlyContinue
    }
    New-Item -ItemType Directory -Force -Path $RuntimeDir | Out-Null
    $manifest = @("# HEYU voice runtime - whisper.cpp $RT_TAG ($($r.File), sha256 $($r.Sha))", "# MIT License (c) The ggml authors - see THIRD_PARTY_NOTICES.md")
    foreach ($f in $files) {
        Copy-Item -Force -LiteralPath $f.FullName -Destination (Join-Path $RuntimeDir $f.Name)
        $manifest += "$(Get-Sha256 $f.FullName)  $($f.Name)"
    }
    Set-Content -LiteralPath (Join-Path $RuntimeDir 'MANIFEST.sha256') -Value $manifest -Encoding UTF8
    Remove-Item -Recurse -Force -LiteralPath $tmp
    Say "  ✔ 运行库已安装到：$RuntimeDir" 'Green'
}

Say "================================================================"
Say " 鹤羽音形 HEYU · 离线语音输入 · 模型下载器（唯一联网组件，手动运行）"
Say "================================================================"
if ($List) { Show-Status; exit 0 }

$doModel = $PSBoundParameters.ContainsKey('Model') -or -not $Runtime
$doRuntime = $Runtime -or -not $PSBoundParameters.ContainsKey('Model')
if (-not $Model) { $Model = 'large-v3-turbo-q5_0' }

if ($doModel) { Install-Model $Model }
if ($doRuntime) { Install-Runtime $Variant }

Say ""
Show-Status
Say ""
Say "完成！之后语音识别完全离线运行。按 Ctrl+Alt+V 开始/结束说话（可在 HEYU 控制台 → 语音输入 中修改）。" 'Green'
exit 0
