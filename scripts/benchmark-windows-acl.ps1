# Isolated diagnostic only. Never use these environment variants in production
# without a separate review of the observed results and required variables.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$jobClock = [Diagnostics.Stopwatch]::StartNew()
$root = $null

function Write-Result([string]$label, [long]$elapsedMs, $exitCode) {
    [ordered]@{ case = $label; elapsed_ms = $elapsedMs; exit = $exitCode } |
        ConvertTo-Json -Compress | Write-Output
}

try {
    $system = [Environment]::GetEnvironmentVariable('SystemRoot')
    if ([string]::IsNullOrEmpty($system)) { throw 'missing system root' }
    $program = Join-Path $system 'System32/WindowsPowerShell/v1.0/powershell.exe'
    $source = Join-Path $PSScriptRoot '../crates/desktop-agent/src/private_acl.rs'
    $contents = [IO.File]::ReadAllText($source)
    $matches = [regex]::Matches($contents, 'let script = r#"(?<body>.*?)"#;', 'Singleline')
    if ($matches.Count -ne 1) { throw 'fixed ACL script unavailable' }
    $aclScript = $matches[0].Groups['body'].Value

    $root = Join-Path ([IO.Path]::GetTempPath()) ('aterminal-acl-benchmark-' + [Guid]::NewGuid().ToString('N'))
    $null = [IO.Directory]::CreateDirectory($root)
    $temporary = Join-Path $root 'temporary'
    $null = [IO.Directory]::CreateDirectory($temporary)

    # Reverse mode order between baselines and ACLs to make a simple cold-start
    # explanation less likely. This is one pass, not a statistical benchmark.
    $cases = @(
        @{ Label = 'noop-cleared'; Mode = 'cleared'; Acl = $false },
        @{ Label = 'noop-inherited'; Mode = 'inherited'; Acl = $false },
        @{ Label = 'noop-minimal'; Mode = 'minimal'; Acl = $false },
        @{ Label = 'acl-minimal'; Mode = 'minimal'; Acl = $true },
        @{ Label = 'acl-inherited'; Mode = 'inherited'; Acl = $true },
        @{ Label = 'acl-cleared'; Mode = 'cleared'; Acl = $true }
    )
    foreach ($case in $cases) {
        # Six subprocesses fit in four minutes at their hard ceiling. Leave
        # setup/cleanup margin inside the workflow's five-minute job limit.
        if ($jobClock.Elapsed.TotalSeconds -gt 210) {
            Write-Result $case.Label 0 'budget-not-run'
            continue
        }
        $leaf = Join-Path $root ([Guid]::NewGuid().ToString('N'))
        $null = [IO.Directory]::CreateDirectory($leaf)
        $start = [Diagnostics.ProcessStartInfo]::new()
        $start.FileName = $program
        $start.WorkingDirectory = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../crates/desktop-cli'))
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.RedirectStandardInput = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        foreach ($argument in @('-NoLogo', '-NoProfile', '-NonInteractive', '-Command')) {
            $start.ArgumentList.Add($argument)
        }
        $start.ArgumentList.Add($(if ($case.Acl) { $aclScript } else { 'exit 0' }))

        if ($case.Mode -ne 'inherited') { $start.Environment.Clear() }
        $start.Environment['SystemRoot'] = $system
        if ($case.Mode -eq 'minimal') {
            foreach ($name in @('WINDIR', 'SystemDrive', 'COMSPEC', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA')) {
                $value = [Environment]::GetEnvironmentVariable($name)
                if (-not [string]::IsNullOrEmpty($value)) { $start.Environment[$name] = $value }
            }
            $start.Environment['TEMP'] = $temporary
            $start.Environment['TMP'] = $temporary
            $start.Environment['PATH'] = "$system\System32;$system;$system\System32\Wbem;$system\System32\WindowsPowerShell\v1.0"
            $start.Environment['PATHEXT'] = '.COM;.EXE;.BAT;.CMD'
        }
        $start.Environment['ATERMINAL_ACL_PATH'] = $leaf
        $start.Environment['ATERMINAL_ACL_CREATE'] = '1'
        $start.Environment['ATERMINAL_ACL_DIRECTORY'] = '1'
        # Suppress the optional timing trace even if the runner inherited it.
        $start.Environment.Remove('ATERMINAL_STARTUP_TRACE') | Out-Null
        $start.Environment.Remove('ATERMINAL_ACL_TRACE') | Out-Null

        $process = [Diagnostics.Process]::new()
        $process.StartInfo = $start
        $clock = [Diagnostics.Stopwatch]::StartNew()
        $exit = 'launch-error'
        try {
            if (-not $process.Start()) { throw 'process launch failed' }
            $process.StandardInput.Close()
            # Drain without storing or publishing any output, paths, or ACL data.
            $stdout = $process.StandardOutput.BaseStream.CopyToAsync([IO.Stream]::Null)
            $stderr = $process.StandardError.BaseStream.CopyToAsync([IO.Stream]::Null)
            # Reserve one second for termination/draining, keeping each case
            # bounded to approximately forty seconds including cleanup.
            $remainingMs = [Math]::Max(1, 39000 - [int]$clock.ElapsedMilliseconds)
            if ($process.WaitForExit($remainingMs)) {
                $exit = $process.ExitCode
                $drains = [Threading.Tasks.Task[]]@($stdout, $stderr)
                $null = [Threading.Tasks.Task]::WaitAll($drains, 1000)
            } else {
                $exit = 'timeout'
                $process.Kill($true)
                $null = $process.WaitForExit(1000)
            }
        } catch {
            $exit = 'harness-error'
            try {
                if (-not $process.HasExited) { $process.Kill($true) }
            } catch {}
        } finally {
            $clock.Stop()
            $process.Dispose()
            Write-Result $case.Label $clock.ElapsedMilliseconds $exit
        }
    }
} catch {
    Write-Result 'benchmark-setup' $jobClock.ElapsedMilliseconds 'harness-error'
    exit 1
} finally {
    if ($null -ne $root) {
        try { [IO.Directory]::Delete($root, $true) } catch {
            Write-Result 'benchmark-cleanup' $jobClock.ElapsedMilliseconds 'harness-error'
        }
    }
}
