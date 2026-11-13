param([Parameter(Mandatory)][ValidateSet('setup','python','core')][string]$Check)
$ErrorActionPreference='Stop'
if($PSVersionTable.PSEdition -ne 'Core' -or $PSVersionTable.PSVersion.Major -ne 7){throw 'PowerShell 7 is required.'}
switch($Check){
  setup {
    if(-not $env:WCS_TEST_ROOT -or -not $env:WCS_DATA_DIR){throw 'Isolated test and data directories are required.'}
    & rustup.exe toolchain install 1.90.0 --profile minimal
    if($LASTEXITCODE -ne 0){exit $LASTEXITCODE}
    & rustup.exe default 1.90.0
    if($LASTEXITCODE -ne 0){exit $LASTEXITCODE}
    New-Item -ItemType Directory -Force -Path $env:WCS_TEST_ROOT,$env:WCS_DATA_DIR | Out-Null
  }
  python {& python.exe -B -m unittest discover -s tests -p 'test_*.py';exit $LASTEXITCODE}
  core {& cargo.exe test --manifest-path core/Cargo.toml --locked --features downloads;exit $LASTEXITCODE}
}
