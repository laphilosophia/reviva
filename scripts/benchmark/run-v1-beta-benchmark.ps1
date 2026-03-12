param(
    [Parameter(Mandatory = $true)]
    [string]$TargetRepo,
    [string]$IncrementalFrom = "HEAD~1",
    [string]$OutputJson = "",
    [string]$OutputMarkdown = ""
)

$ErrorActionPreference = "Stop"

function Get-WarningValue {
    param(
        [Parameter(Mandatory = $true)]
        [array]$Warnings,
        [Parameter(Mandatory = $true)]
        [string]$Prefix
    )
    foreach ($warning in $Warnings) {
        if ($warning -is [string] -and $warning.StartsWith($Prefix, [System.StringComparison]::Ordinal)) {
            return $warning.Substring($Prefix.Length)
        }
    }
    return $null
}

function Run-RevivaReview {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Scenario,
        [Parameter(Mandatory = $true)]
        [string]$SessionId,
        [Parameter(Mandatory = $true)]
        [string[]]$Args,
        [Parameter(Mandatory = $true)]
        [string]$RevivaBin,
        [Parameter(Mandatory = $true)]
        [string]$GitConfigPath,
        [Parameter(Mandatory = $true)]
        [string]$RepoPath
    )

    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $env:REVIVA_TEST_SESSION_ID = $SessionId
    $env:REVIVA_TEST_TIMESTAMP = $timestamp

    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        Push-Location $RepoPath
        try {
            $env:GIT_CONFIG_GLOBAL = $GitConfigPath
            & $RevivaBin review @Args | Out-Null
            if ($LASTEXITCODE -ne 0) {
                throw "reviva review failed for scenario '$Scenario' (exit=$LASTEXITCODE)"
            }
        }
        finally {
            Remove-Item Env:GIT_CONFIG_GLOBAL -ErrorAction SilentlyContinue
            Pop-Location
        }
    }
    finally {
        $stopwatch.Stop()
        Remove-Item Env:REVIVA_TEST_SESSION_ID -ErrorAction SilentlyContinue
        Remove-Item Env:REVIVA_TEST_TIMESTAMP -ErrorAction SilentlyContinue
    }

    $sessionPath = Join-Path $RepoPath ".reviva\sessions\$SessionId.json"
    if (-not (Test-Path $sessionPath)) {
        throw "session artifact not found: $sessionPath"
    }

    $session = Get-Content $sessionPath -Raw | ConvertFrom-Json
    $warnings = @($session.warnings)

    return [PSCustomObject]@{
        scenario                              = $Scenario
        session_id                            = $SessionId
        elapsed_ms                            = [int]$stopwatch.ElapsedMilliseconds
        findings_count                        = @($session.findings).Count
        review_cache                          = Get-WarningValue -Warnings $warnings -Prefix "review_cache="
        review_cache_source                   = Get-WarningValue -Warnings $warnings -Prefix "review_cache_source="
        incremental_scope                     = Get-WarningValue -Warnings $warnings -Prefix "incremental_scope="
        incremental_file_count                = Get-WarningValue -Warnings $warnings -Prefix "incremental_file_count="
        incremental_fallback_full_file_count  = Get-WarningValue -Warnings $warnings -Prefix "incremental_fallback_full_file_count="
    }
}

$workspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$resolvedRepoPath = (Resolve-Path $TargetRepo).Path
$configPath = Join-Path $resolvedRepoPath ".reviva\config.toml"
if (-not (Test-Path $configPath)) {
    throw "missing config: $configPath (review backend settings must be configured before benchmark)"
}

if ([string]::IsNullOrWhiteSpace($OutputJson)) {
    $OutputJson = Join-Path $workspaceRoot "docs\artifacts\v1-beta-benchmark.json"
}
if ([string]::IsNullOrWhiteSpace($OutputMarkdown)) {
    $OutputMarkdown = Join-Path $workspaceRoot "docs\artifacts\v1-beta-benchmark.md"
}
$gitConfigPath = Join-Path $workspaceRoot ".tmp-benchmark-gitconfig"
if (-not (Test-Path $gitConfigPath)) {
    New-Item -ItemType File -Path $gitConfigPath -Force | Out-Null
}
& git config --file $gitConfigPath --add safe.directory $resolvedRepoPath | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "unable to prepare temporary git config for safe.directory (exit=$LASTEXITCODE)"
}

Push-Location $workspaceRoot
try {
    & cargo build --quiet -p reviva-cli | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "failed to build reviva-cli binary (exit=$LASTEXITCODE)"
    }
}
finally {
    Pop-Location
}

$revivaBin = Join-Path $workspaceRoot "target\debug\reviva.exe"
if (-not (Test-Path $revivaBin)) {
    throw "reviva binary not found after build: $revivaBin"
}

$outputJsonDir = Split-Path -Parent $OutputJson
$outputMdDir = Split-Path -Parent $OutputMarkdown
if (-not (Test-Path $outputJsonDir)) {
    New-Item -ItemType Directory -Path $outputJsonDir -Force | Out-Null
}
if (-not (Test-Path $outputMdDir)) {
    New-Item -ItemType Directory -Path $outputMdDir -Force | Out-Null
}

$runId = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds().ToString()

$fullMissArgs = @(
    "--repo", $resolvedRepoPath,
    "--mode", "launch-readiness",
    "--profile", "launch-readiness",
    "--note", "benchmark_run=$runId scenario=full",
    "--file", "packages/core/src/core/agent.ts"
)
$fullSetMissArgs = @(
    "--repo", $resolvedRepoPath,
    "--mode", "launch-readiness",
    "--profile", "launch-readiness",
    "--note", "benchmark_run=$runId scenario=full_set",
    "--file", "package.json",
    "--file", "packages/cli/package.json",
    "--file", "packages/core/package.json",
    "--file", "packages/express/package.json",
    "--file", "packages/fastify/package.json"
)
$incrementalMissArgs = @(
    "--repo", $resolvedRepoPath,
    "--mode", "launch-readiness",
    "--profile", "launch-readiness",
    "--note", "benchmark_run=$runId scenario=incremental",
    "--incremental-from", $IncrementalFrom
)

$results = @()
$results += Run-RevivaReview -Scenario "full_miss" -SessionId "m6p2-full-miss-1" -Args $fullMissArgs -RevivaBin $revivaBin -GitConfigPath $gitConfigPath -RepoPath $resolvedRepoPath
$results += Run-RevivaReview -Scenario "full_hit" -SessionId "m6p2-full-hit-1" -Args $fullMissArgs -RevivaBin $revivaBin -GitConfigPath $gitConfigPath -RepoPath $resolvedRepoPath
$results += Run-RevivaReview -Scenario "full_set_miss" -SessionId "m6p2-fullset-miss-1" -Args $fullSetMissArgs -RevivaBin $revivaBin -GitConfigPath $gitConfigPath -RepoPath $resolvedRepoPath
$results += Run-RevivaReview -Scenario "incremental_miss" -SessionId "m6p2-incremental-miss-1" -Args $incrementalMissArgs -RevivaBin $revivaBin -GitConfigPath $gitConfigPath -RepoPath $resolvedRepoPath

$fullMiss = $results | Where-Object { $_.scenario -eq "full_miss" } | Select-Object -First 1
$fullHit = $results | Where-Object { $_.scenario -eq "full_hit" } | Select-Object -First 1
$fullSetMiss = $results | Where-Object { $_.scenario -eq "full_set_miss" } | Select-Object -First 1
$incrementalMiss = $results | Where-Object { $_.scenario -eq "incremental_miss" } | Select-Object -First 1

if ($fullMiss.review_cache -ne "miss") {
    throw "benchmark expectation failed: full_miss must be review_cache=miss (got '$($fullMiss.review_cache)')"
}
if ($fullHit.review_cache -ne "hit") {
    throw "benchmark expectation failed: full_hit must be review_cache=hit (got '$($fullHit.review_cache)')"
}
if ($fullSetMiss.review_cache -ne "miss") {
    throw "benchmark expectation failed: full_set_miss must be review_cache=miss (got '$($fullSetMiss.review_cache)')"
}
if ($incrementalMiss.review_cache -ne "miss") {
    throw "benchmark expectation failed: incremental_miss must be review_cache=miss (got '$($incrementalMiss.review_cache)')"
}

$cacheGainPct = $null
if ($fullMiss.elapsed_ms -gt 0) {
    $cacheGainPct = [Math]::Round((1 - ($fullHit.elapsed_ms / [double]$fullMiss.elapsed_ms)) * 100, 2)
}
$incrementalGainPct = $null
if ($fullSetMiss.elapsed_ms -gt 0) {
    $incrementalGainPct = [Math]::Round((1 - ($incrementalMiss.elapsed_ms / [double]$fullSetMiss.elapsed_ms)) * 100, 2)
}

$report = [PSCustomObject]@{
    generated_at_utc              = (Get-Date).ToUniversalTime().ToString("o")
    target_repo                   = $resolvedRepoPath
    incremental_from              = $IncrementalFrom
    scenarios                     = $results
    derived_metrics               = [PSCustomObject]@{
        cache_gain_percent            = $cacheGainPct
        incremental_gain_percent      = $incrementalGainPct
    }
}

$report | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputJson -Encoding UTF8

$markdown = @()
$markdown += "# Reviva v1-beta Benchmark Artifact"
$markdown += ""
$markdown += "- Generated At (UTC): $($report.generated_at_utc)"
$markdown += "- Target Repo: $($report.target_repo)"
$markdown += "- Incremental From: $($report.incremental_from)"
$markdown += ""
$markdown += "## Scenario Results"
$markdown += ""
$markdown += "| Scenario | Session ID | Elapsed (ms) | Cache | Cache Source | Incremental Scope | Incremental Files | Fallback Full Files | Findings |"
$markdown += "| --- | --- | ---: | --- | --- | --- | ---: | ---: | ---: |"
foreach ($result in $results) {
    $markdown += "| $($result.scenario) | $($result.session_id) | $($result.elapsed_ms) | $($result.review_cache) | $($result.review_cache_source) | $($result.incremental_scope) | $($result.incremental_file_count) | $($result.incremental_fallback_full_file_count) | $($result.findings_count) |"
}
$markdown += ""
$markdown += "## Derived Metrics"
$markdown += ""
$markdown += "- Cache gain percent: $($report.derived_metrics.cache_gain_percent)"
$markdown += "- Incremental gain percent: $($report.derived_metrics.incremental_gain_percent)"
$markdown += ""
$markdown += "## Scope Note"
$markdown += ""
$markdown += "- incremental_scope=diff_hunks means only git diff hunks are sent."
$markdown += "- incremental_fallback_full_file_count>0 means those files were reviewed with full file content."
$markdown += ""

$markdown -join "`n" | Set-Content -Path $OutputMarkdown -Encoding UTF8

Write-Host "benchmark json: $OutputJson"
Write-Host "benchmark markdown: $OutputMarkdown"

if (Test-Path $gitConfigPath) {
    Remove-Item $gitConfigPath -Force -ErrorAction SilentlyContinue
}
