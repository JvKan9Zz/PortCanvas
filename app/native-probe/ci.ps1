param([ValidateSet('setup','test','build')][string]$Check)
$ErrorActionPreference='Stop'
if($PSVersionTable.PSEdition -ne 'Core' -or $PSVersionTable.PSVersion.Major -ne 7){throw 'PowerShell 7 required'}
switch($Check){
    setup {
        (Get-Process -Id $PID).Path
        $PSVersionTable
        & rustup.exe toolchain install 1.90.0 --profile minimal
        if($LASTEXITCODE -ne 0){exit $LASTEXITCODE}
        if(-not $env:WCS_TEST_ROOT){throw 'Isolated WCS_TEST_ROOT required'}
        New-Item -ItemType Directory -Force -Path $env:WCS_TEST_ROOT|Out-Null
    }
    test {& cargo.exe +1.90.0 test --locked;exit $LASTEXITCODE}
    build {& cargo.exe +1.90.0 build --locked --features gui;exit $LASTEXITCODE}
}
