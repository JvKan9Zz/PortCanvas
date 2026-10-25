"""Verify the real DMG and a bounded native launch in an isolated data directory."""
from pathlib import Path
import hashlib,json,os,plistlib,shutil,subprocess,sys,time,platform

assert sys.platform=='darwin'
source=Path(sys.argv[1]).resolve();output=Path(sys.argv[2]).resolve()
packages=list(source.glob('*.dmg'));assert len(packages)==1,packages
package=packages[0];output.mkdir(parents=True,exist_ok=True)
target=output/'PortCanvas_0.5.6_universal.dmg'
if package!=target:shutil.copy2(package,target)
def run(*args):return subprocess.check_output(args,text=True,stderr=subprocess.STDOUT,timeout=40).strip()
run('hdiutil','verify',str(target))
mount=output/'mounted';mount.mkdir(exist_ok=True)
run('hdiutil','attach','-readonly','-nobrowse','-mountpoint',str(mount),str(target))
process=None
try:
    apps=list(mount.glob('*.app'));assert len(apps)==1
    application=apps[0];info=plistlib.loads((application/'Contents/Info.plist').read_bytes())
    assert info['CFBundleShortVersionString']=='0.5.6'
    binary=application/'Contents/MacOS'/info['CFBundleExecutable']
    arch=run('lipo','-archs',str(binary));assert {'x86_64','arm64'}<=set(arch.split())
    signature=run('codesign','--verify','--deep','--strict','--verbose=2',str(application))
    details=run('codesign','-dv','--verbose=2',str(application));assert 'Signature=adhoc' in details
    data=output/'runtime-data';data.mkdir(exist_ok=True)
    env=os.environ.copy();env['WCS_DATA_DIR']=str(data);env['WCS_SESSION_NAMESPACE']='v056-ci-smoke'
    with (output/'native-launch.log').open('wb') as log:
        process=subprocess.Popen([str(binary)],env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=log)
        time.sleep(8)
        assert process.poll() is None,(output/'native-launch.log').read_text()
        capture=subprocess.run(['screencapture','-x',str(output/'native-launch.png')],capture_output=True,timeout=10)
        report={'version':'0.5.6','hostArchitecture':platform.machine(),'binaryArchitectures':arch.split(),'adHocSignatureVerified':True,'notarized':False,'dmgVerified':True,'nativeProcessSurvivedEightSeconds':True,'screenshotCaptured':capture.returncode==0,'deviceConnectionTested':False,'packageSha256':hashlib.sha256(target.read_bytes()).hexdigest()}
        (output/'package-check.json').write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps(report))
finally:
    if process is not None and process.poll() is None:
        process.terminate()
        try:process.wait(timeout=5)
        except subprocess.TimeoutExpired:process.kill();process.wait(timeout=5)
    run('hdiutil','detach',str(mount))
