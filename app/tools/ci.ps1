param(
    [Parameter(Mandatory)]
    [ValidateSet('setup', 'install', 'frontend', 'typescript', 'frontend-tests', 'python', 'core', 'native', 'ssh', 'compile')]
    [string] $Check
)

$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSEdition -ne 'Core' -or $PSVersionTable.PSVersion.Major -ne 7) {
    throw 'PowerShell 7 is required.'
}

function Invoke-Checked([string] $Program, [string[]] $CommandArgs) {
    & $Program @CommandArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

switch ($Check) {
    'setup' {
        $PSVersionTable
        [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
        if (-not $env:WCS_TEST_ROOT -or -not $env:WCS_DATA_DIR) { throw 'Isolated test and data directories are required.' }
        Invoke-Checked 'rustup.exe' @('toolchain', 'install', '1.90.0', '--profile', 'minimal')
        Invoke-Checked 'rustup.exe' @('default', '1.90.0')
        New-Item -ItemType Directory -Force -Path $env:WCS_TEST_ROOT, $env:WCS_DATA_DIR | Out-Null
    }
    'install' { Invoke-Checked 'npm.cmd' @('ci') }
    'frontend' { Invoke-Checked 'npm.cmd' @('run', 'build') }
    'typescript' { Invoke-Checked 'npx.cmd' @('--no-install', 'tsc', '--noEmit') }
    'frontend-tests' { Invoke-Checked 'npm.cmd' @('run', 'test:frontend') }
    'python' { Invoke-Checked 'python.exe' @('-B', '-m', 'unittest', 'discover', '-s', 'tests', '-p', 'test_*.py') }
    'core' { Invoke-Checked 'cargo.exe' @('test', '--manifest-path', 'core/Cargo.toml', '--locked', '--features', 'downloads', '--lib') }
    'native' { Invoke-Checked 'cargo.exe' @('test', '--manifest-path', 'src-tauri/Cargo.toml', '--locked', '--release', '--lib') }
    'ssh' { Invoke-Checked 'cargo.exe' @('test', '--manifest-path', 'src-tauri/Cargo.toml', '--locked', '--release', '--no-default-features', '--test', 'ssh_lifecycle') }
    'compile' { Invoke-Checked 'npx.cmd' @('--no-install', 'tauri', 'build', '--no-bundle', '--ci') }
}
