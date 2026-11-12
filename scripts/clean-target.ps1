<#
.SYNOPSIS
    Reclaim disk space used by Cargo build artifacts (`target/`).

.DESCRIPTION
    Cargo never garbage-collects artifacts produced for a previous feature set,
    rustflags or toolchain, and MSVC writes one large `.pdb` per linked binary.
    A Rust workspace that is built and tested often therefore accumulates tens of
    gigabytes in `target/` even though only the newest artifacts are current.

    This script reports the size of each `target/` directory and prunes the stale
    parts:

      * incremental-compilation unit/session caches (`target/<profile>/incremental/`)
      * stale artifacts in `target/<profile>/deps/` and `.../examples/`
        (`.pdb`, `.exe`, `.dll`, `.exp`, `.lib`, `.rlib`, `.rmeta`, `.o`)

    Artifacts newer than `-OlderThanDays` are kept, so the last build stays hot.
    Nothing outside `target/` is touched.

.PARAMETER Path
    One or more repository roots (directories that contain `target/`) or
    `target/` directories themselves. Defaults to the current directory.
    Example: `-Path ., ..\rust-ef, ..\rust-webx`

.PARAMETER OlderThanDays
    Only prune files/directories whose last write time is older than this many
    days. Defaults to 14.

.PARAMETER IncrementalOnly
    Remove the whole `target/<profile>/incremental` tree without a full rebuild.
    This is the safest prune: incremental caches are a pure cache.

.PARAMETER All
    Remove entire `target/` directories (`cargo clean` equivalent). Everything is
    rebuilt on the next `cargo build` / `cargo test`.

.PARAMETER Force
    Allow pruning while a `cargo`/`rustc` process is running (not recommended).

.EXAMPLE
    # Report-only: show what would be reclaimed
    ./scripts/clean-target.ps1 -Path ., ../rust-ef, ../rust-webx -WhatIf

.EXAMPLE
    # Prune artifacts older than 14 days in all three framework repos
    ./scripts/clean-target.ps1 -Path ., ../rust-ef, ../rust-webx

.EXAMPLE
    # Full clean (next build recompiles everything)
    ./scripts/clean-target.ps1 -All
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [string[]]$Path = @('.'),
    [int]$OlderThanDays = 14,
    [switch]$IncrementalOnly,
    [switch]$All,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

# `powershell -File script.ps1 -Path .,..\rust-ef` passes the whole list as one
# comma-separated string; expand it so both interactive calls (real arrays) and
# `-File` calls (single string) work.
$expandedPath = @()
foreach ($p in $Path) {
    if (-not (Test-Path -LiteralPath $p) -and $p -match ',') {
        $expandedPath += [regex]::Split($p, '\s*,\s*') | Where-Object { $_ -ne '' }
    }
    else {
        $expandedPath += $p
    }
}
$Path = $expandedPath

# Extensions that make up the bulk of `target/` and are safe to prune: Cargo
# rebuilds them whenever they are missing and a unit is needed again.
$artifactExtensions = @('.pdb', '.exe', '.dll', '.exp', '.lib', '.rlib', '.rmeta', '.o')

function Get-ItemSizeBytes {
    param([System.IO.FileSystemInfo]$Item)
    if ($Item -is [System.IO.FileInfo]) {
        return [long]$Item.Length
    }
    $sum = (Get-ChildItem -LiteralPath $Item.FullName -Recurse -Force -File -ErrorAction SilentlyContinue |
        Measure-Object -Property Length -Sum).Sum
    if ($null -eq $sum) { return [long]0 }
    return [long]$sum
}

function Get-DirSizeBytes {
    param([string]$Directory)
    if (-not (Test-Path -LiteralPath $Directory)) { return [long]0 }
    $sum = (Get-ChildItem -LiteralPath $Directory -Recurse -Force -File -ErrorAction SilentlyContinue |
        Measure-Object -Property Length -Sum).Sum
    if ($null -eq $sum) { return [long]0 }
    return [long]$sum
}

function Format-Gb {
    param([long]$Bytes)
    return ('{0:N2} GB' -f ($Bytes / 1GB))
}

function Resolve-TargetDir {
    param([string]$InputPath)

    $full = (Resolve-Path -LiteralPath $InputPath).ProviderPath
    if (Test-Path -LiteralPath (Join-Path $full 'target')) {
        return (Join-Path $full 'target')
    }
    if ((Split-Path -Leaf $full) -eq 'target' -and (Test-Path -LiteralPath $full)) {
        return $full
    }
    return $null
}

# A prune that races a running build can leave the target directory in a state
# that requires `cargo clean` afterwards; refuse unless the caller insists.
$runningBuild = @(Get-Process -Name cargo, rustc -ErrorAction SilentlyContinue)
if ($runningBuild.Count -gt 0 -and -not $Force -and -not $WhatIfPreference) {
    Write-Warning ("cargo/rustc is running ({0} process(es)). Stop the build first, or re-run with -Force." -f $runningBuild.Count)
    exit 2
}

$cutoff = (Get-Date).AddDays(-[Math]::Max(0, $OlderThanDays))
$totalBefore = [long]0
$totalAfter = [long]0
$totalReclaimed = [long]0
$processed = 0

foreach ($inputPath in $Path) {
    if (-not (Test-Path -LiteralPath $inputPath)) {
        Write-Warning "skip '$inputPath' (does not exist)"
        continue
    }

    $targetDir = Resolve-TargetDir -InputPath $inputPath
    if ($null -eq $targetDir) {
        Write-Warning "skip '$inputPath' (no target/ directory)"
        continue
    }
    $processed++

    $before = Get-DirSizeBytes -Directory $targetDir
    $totalBefore += $before

    Write-Host ''
    Write-Host "== $targetDir ==" -ForegroundColor Cyan
    Write-Host ("   current size: {0}" -f (Format-Gb $before))

    if ($All) {
        if ($PSCmdlet.ShouldProcess($targetDir, 'Remove entire target directory')) {
            Remove-Item -LiteralPath $targetDir -Recurse -Force -ErrorAction Stop
            $totalReclaimed += $before
            Write-Host ("   removed: {0} (whole target/; next build recompiles everything)" -f (Format-Gb $before)) -ForegroundColor Green
            $totalAfter += 0
        }
        else {
            Write-Host ("   would remove: {0} (whole target/)" -f (Format-Gb $before)) -ForegroundColor Yellow
            $totalAfter += $before
        }
        continue
    }

    # Profile directories: debug, release, custom profiles, ...
    $profileDirs = @(Get-ChildItem -LiteralPath $targetDir -Directory -Force -ErrorAction SilentlyContinue)

    if ($IncrementalOnly) {
        $incrementalDirs = @($profileDirs | ForEach-Object {
                $inc = Join-Path $_.FullName 'incremental'
                if (Test-Path -LiteralPath $inc) { Get-Item -LiteralPath $inc -Force }
            })
        if ($incrementalDirs.Count -eq 0) {
            Write-Host '   no incremental cache found' -ForegroundColor DarkGray
        }
        else {
            $size = [long]0
            foreach ($inc in $incrementalDirs) { $size += Get-ItemSizeBytes -Item $inc }
            if ($PSCmdlet.ShouldProcess($targetDir, ("Remove {0} incremental cache directory/directories" -f $incrementalDirs.Count))) {
                foreach ($inc in $incrementalDirs) {
                    Remove-Item -LiteralPath $inc.FullName -Recurse -Force -ErrorAction SilentlyContinue
                }
                $totalReclaimed += $size
                Write-Host ("   removed incremental cache: {0}" -f (Format-Gb $size)) -ForegroundColor Green
            }
            else {
                Write-Host ("   would remove incremental cache: {0}" -f (Format-Gb $size)) -ForegroundColor Yellow
            }
        }
        $after = Get-DirSizeBytes -Directory $targetDir
        $totalAfter += $after
        Write-Host ("   size after: {0}" -f (Format-Gb $after))
        continue
    }

    # ---- plan the prune -------------------------------------------------
    $incrementalPlan = New-Object System.Collections.Generic.List[System.IO.FileSystemInfo]
    foreach ($profile in $profileDirs) {
        $inc = Join-Path $profile.FullName 'incremental'
        if (-not (Test-Path -LiteralPath $inc)) { continue }
        foreach ($unit in @(Get-ChildItem -LiteralPath $inc -Directory -Force -ErrorAction SilentlyContinue)) {
            if ($unit.LastWriteTime -lt $cutoff) {
                # Whole unit cache is stale (old feature set / rustflags / toolchain).
                $incrementalPlan.Add($unit)
            }
            else {
                # Unit is current, but it may hold older session directories.
                foreach ($session in @(Get-ChildItem -LiteralPath $unit.FullName -Directory -Force -ErrorAction SilentlyContinue)) {
                    if ($session.LastWriteTime -lt $cutoff) { $incrementalPlan.Add($session) }
                }
            }
        }
    }

    $artifactPlan = New-Object System.Collections.Generic.List[System.IO.FileInfo]
    foreach ($profile in $profileDirs) {
        foreach ($name in @('deps', 'examples')) {
            $dir = Join-Path $profile.FullName $name
            if (-not (Test-Path -LiteralPath $dir)) { continue }
            foreach ($file in @(Get-ChildItem -LiteralPath $dir -File -Force -ErrorAction SilentlyContinue)) {
                if (($artifactExtensions -contains $file.Extension) -and ($file.LastWriteTime -lt $cutoff)) {
                    $artifactPlan.Add($file)
                }
            }
        }
    }

    $incrementalBytes = [long]0
    foreach ($item in $incrementalPlan) { $incrementalBytes += Get-ItemSizeBytes -Item $item }
    $artifactBytes = [long]0
    foreach ($item in $artifactPlan) { $artifactBytes += [long]$item.Length }
    $plannedBytes = $incrementalBytes + $artifactBytes
    $plannedCount = $incrementalPlan.Count + $artifactPlan.Count

    if ($plannedCount -eq 0) {
        Write-Host ("   nothing older than {0} days; build artifacts are current" -f $OlderThanDays) -ForegroundColor DarkGray
    }
    elseif ($PSCmdlet.ShouldProcess($targetDir, ("Prune {0} stale artifacts ({1})" -f $plannedCount, (Format-Gb $plannedBytes)))) {
        $removed = [long]0
        $failed = 0
        foreach ($item in $incrementalPlan) {
            $size = Get-ItemSizeBytes -Item $item
            try {
                Remove-Item -LiteralPath $item.FullName -Recurse -Force -ErrorAction Stop
                $removed += $size
            }
            catch { $failed++ }
        }
        foreach ($item in $artifactPlan) {
            $size = [long]$item.Length
            try {
                Remove-Item -LiteralPath $item.FullName -Force -ErrorAction Stop
                $removed += $size
            }
            catch { $failed++ }
        }
        $totalReclaimed += $removed
        Write-Host ("   pruned incremental: {0} item(s), {1}" -f $incrementalPlan.Count, (Format-Gb $incrementalBytes))
        Write-Host ("   pruned artifacts:   {0} file(s), {1}" -f $artifactPlan.Count, (Format-Gb $artifactBytes)) -ForegroundColor Green
        if ($failed -gt 0) {
            Write-Warning ("{0} item(s) could not be removed (in use?); re-run after the next build finishes" -f $failed)
        }
    }
    else {
        Write-Host ("   would prune incremental: {0} item(s), {1}" -f $incrementalPlan.Count, (Format-Gb $incrementalBytes)) -ForegroundColor Yellow
        Write-Host ("   would prune artifacts:   {0} file(s), {1}" -f $artifactPlan.Count, (Format-Gb $artifactBytes)) -ForegroundColor Yellow
        $totalReclaimed += $plannedBytes
    }

    $after = Get-DirSizeBytes -Directory $targetDir
    $totalAfter += $after
    Write-Host ("   size after: {0}" -f (Format-Gb $after))
}

if ($processed -eq 0) {
    Write-Warning 'no target/ directory found in the given paths'
    exit 1
}

Write-Host ''
Write-Host '== summary ==' -ForegroundColor Cyan
if ($WhatIfPreference) {
    Write-Host ("   would reclaim: {0}  (current total: {1})" -f (Format-Gb $totalReclaimed), (Format-Gb $totalBefore))
}
else {
    Write-Host ("   reclaimed: {0}  ({1} -> {2})" -f (Format-Gb $totalReclaimed), (Format-Gb $totalBefore), (Format-Gb $totalAfter))
}
Write-Host '   tip: run `cargo test --workspace` once afterwards to confirm the workspace still builds.' -ForegroundColor DarkGray
