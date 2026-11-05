#requires -Version 7.5
# Private adapters remain available to invocation-time alias bodies.
function global:__shed_text([string] $value) {
    if ($value.Contains([char]0)) { throw 'NUL in runtime value' }
    [void][Text.UTF8Encoding]::new($false, $true).GetByteCount($value)
    return $value
}

function global:__shed_env([string] $name) {
    return __shed_text ([string][Environment]::GetEnvironmentVariable($name))
}

function global:__shed_home {
    $value = __shed_text ([string]$HOME)
    if (-not [IO.Path]::IsPathFullyQualified($value) -or -not [IO.Directory]::Exists($value)) {
        throw 'home is not an absolute directory'
    }
    return $value
}

function global:__shed_cwd {
    $value = __shed_text $ExecutionContext.SessionState.Path.CurrentFileSystemLocation.ProviderPath
    if (-not [IO.Path]::IsPathFullyQualified($value) -or -not [IO.Directory]::Exists($value)) {
        throw 'working directory is unavailable'
    }
    return $value
}

function global:__shed_absolute([string] $value) {
    if ([IO.Path]::IsPathFullyQualified($value)) { return $value }
    $cwd = __shed_cwd
    if ([IO.Path]::IsPathRooted($value)) { return [IO.Path]::GetFullPath($value, $cwd) }
    return [IO.Path]::Join($cwd, $value)
}

function global:__shed_exists([string] $value) {
    if ($value.Length -eq 0) { return $false }
    $value = __shed_absolute $value
    return [IO.File]::Exists($value) -or [IO.Directory]::Exists($value)
}

function global:__shed_path([string] $value, [bool] $prepend, [char] $delimiter) {
    if ($value.Length -eq 0 -or $value.Contains($delimiter)) { throw 'invalid PATH operand' }
    $value = __shed_absolute $value
    if ($value.Contains($delimiter)) { throw 'absolute path contains PATH delimiter' }
    $old = [Environment]::GetEnvironmentVariable('PATH')
    $entries = [Collections.Generic.List[string]]::new()
    if ($prepend) { $entries.Add($value) }
    if ($null -ne $old) {
        foreach ($entry in (__shed_text $old).Split($delimiter)) {
            if (-not $value.Equals($entry, [StringComparison]::Ordinal)) { $entries.Add($entry) }
        }
    }
    if (-not $prepend) { $entries.Add($value) }
    [Environment]::SetEnvironmentVariable('PATH', [string]::Join($delimiter, $entries))
}

function global:__shed_resolve([string] $name) {
    if ($name.Length -eq 0) { return $null }
    # Discovery reads the runspace/global preference, not a function-local one.
    # Own the temporary change and restore even an originally absent variable.
    $saved = $ExecutionContext.SessionState.PSVariable.Get('global:PSModuleAutoLoadingPreference')
    if ($null -ne $saved) { $savedValue = $saved.Value }
    try {
        $global:PSModuleAutoLoadingPreference = 'None'
        $command = $ExecutionContext.InvokeCommand.GetCommand($name, [System.Management.Automation.CommandTypes]::Application)
    } finally {
        if ($null -eq $saved) {
            $ExecutionContext.SessionState.PSVariable.Remove('global:PSModuleAutoLoadingPreference')
        } else { $global:PSModuleAutoLoadingPreference = $savedValue }
    }
    if ($null -ne $command -and $command.CommandType -eq [System.Management.Automation.CommandTypes]::Application) {
        return $command
    }
    return $null
}

function global:__shed_init([string[]] $values) {
    $program = __shed_resolve $values[0]
    if ($null -eq $program) { throw "external executable not found: $($values[0])" }
    $process = [Diagnostics.Process]::new()
    $buffer = $null
    $errorStream = $null
    try {
        $start = $process.StartInfo
        $start.FileName = $program.Path
        $start.UseShellExecute = $false
        $start.WorkingDirectory = __shed_cwd
        for ($i = 1; $i -lt $values.Length; $i++) { $start.ArgumentList.Add($values[$i]) }
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        if (-not $process.Start()) { throw "cannot launch $program" }
        # Start both copies before waiting: a full stderr pipe must not block stdout.
        $buffer = [IO.MemoryStream]::new()
        $stdout = $process.StandardOutput.BaseStream.CopyToAsync($buffer)
        $errorStream = [Console]::OpenStandardError()
        $stderr = $process.StandardError.BaseStream.CopyToAsync($errorStream)
        $process.WaitForExit()
        [void]$stdout.GetAwaiter().GetResult()
        [void]$stderr.GetAwaiter().GetResult()
        if ($process.ExitCode -ne 0) { throw "initializer exited with status $($process.ExitCode)" }
        return __shed_text ([Text.UTF8Encoding]::new($false, $true).GetString($buffer.ToArray()))
    } finally {
        if ($null -ne $buffer) { $buffer.Dispose() }
        if ($null -ne $errorStream) { $errorStream.Dispose() }
        $process.Dispose()
    }
}
