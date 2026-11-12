param([ValidateSet('setup','test','build','smoke','imports')][string]$Check)
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
    build {
        & cargo.exe +1.90.0 build --locked --features gui
        if($LASTEXITCODE -ne 0){exit $LASTEXITCODE}
        & cargo.exe +1.90.0 build --locked --features gui --example glass_poc
        exit $LASTEXITCODE
    }
    smoke {& ./target/debug/PortCanvasNativeProbe.exe --core-smoke;exit $LASTEXITCODE}
    imports {
        $vswhere="${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
        $studio=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if($LASTEXITCODE -ne 0 -or -not $studio){throw 'MSVC installation not found'}
        $version=(Get-Content -LiteralPath (Join-Path $studio 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt') -Raw).Trim()
        $dumpbin=Join-Path $studio "VC/Tools/MSVC/$version/bin/Hostx64/x64/dumpbin.exe"
        $target=if($env:CARGO_TARGET_DIR){$env:CARGO_TARGET_DIR}else{'./target'}
        $imports=& $dumpbin /DEPENDENTS (Join-Path $target 'x86_64-pc-windows-msvc/release/PortCanvasNative.exe')
        if($LASTEXITCODE -ne 0){throw 'Native PE import inspection failed'}
        $imports
        if(($imports -join "`n") -match '(?i)(WebView2Loader|libcef|VCRUNTIME\d|MSVCP\d|MSVCR\d|CONCRT\d)'){throw 'Unexpected browser or external VC runtime import'}
    }
}
