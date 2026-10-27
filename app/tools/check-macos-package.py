"""Verify the real DMG and a bounded native launch in an isolated data directory."""
from pathlib import Path
import hashlib,json,os,plistlib,shutil,subprocess,sys,time,platform

assert sys.platform=='darwin'
source=Path(sys.argv[1]).resolve();output=Path(sys.argv[2]).resolve()
version=json.loads((Path(__file__).resolve().parents[1]/'package.json').read_text())['version']
fixtures=Path(sys.argv[3]).resolve() if len(sys.argv)>3 else None
packages=list(source.glob('*.dmg'));assert len(packages)==1,packages
package=packages[0];output.mkdir(parents=True,exist_ok=True)
target=output/f'PortCanvas_{version}_universal.dmg'
if package!=target:shutil.copy2(package,target)
def run(*args):return subprocess.check_output(args,text=True,stderr=subprocess.STDOUT,timeout=40).strip()
run('hdiutil','verify',str(target))
mount=output/'mounted';mount.mkdir(exist_ok=True)
run('hdiutil','attach','-readonly','-nobrowse','-mountpoint',str(mount),str(target))
process=None
try:
    apps=list(mount.glob('*.app'));assert len(apps)==1
    application=apps[0];info=plistlib.loads((application/'Contents/Info.plist').read_bytes())
    assert info['CFBundleShortVersionString']==version
    assert info.get('LSMinimumSystemVersion')=='13.0'
    assert info.get('NSLocalNetworkUsageDescription')
    assert info.get('NSAppTransportSecurity',{}).get('NSAllowsLocalNetworking') is True
    binary=application/'Contents/MacOS'/info['CFBundleExecutable']
    arch=run('lipo','-archs',str(binary));assert {'x86_64','arm64'}<=set(arch.split())
    signature=run('codesign','--verify','--deep','--strict','--verbose=2',str(application))
    details=run('codesign','-dv','--verbose=2',str(application));assert 'Signature=adhoc' in details
    scenes=['default']+(['image-dark','video-light'] if fixtures else [])
    launches=[]
    for scene in scenes:
        data=output/f'runtime-{scene}';data.mkdir(exist_ok=True)
        settings={'appearance':'dark' if scene=='image-dark' else 'light','glassOpacity':10 if scene!='default' else 100}
        if scene!='default':
            video=scene=='video-light';extension='mp4' if video else 'png';name='05950595059505950595059505950595.'+extension
            backgrounds=data/'backgrounds';backgrounds.mkdir(exist_ok=True)
            shutil.copy2(fixtures/f'background.{extension}',backgrounds/name)
            settings['backgroundVideo' if video else 'backgroundImage']=name
            settings['backgroundVolume']=0;settings['backgroundCrop']={'x':0,'y':0,'width':1,'height':1}
        (data/'settings.json').write_text(json.dumps(settings))
        env=os.environ.copy();env['WCS_DATA_DIR']=str(data);env['WCS_SESSION_NAMESPACE']='v0595-ci-smoke'
        with (output/f'native-{scene}.log').open('wb') as log:
            process=subprocess.Popen([str(binary)],env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=log)
            time.sleep(8)
            assert process.poll() is None,(output/f'native-{scene}.log').read_text()
            capture=subprocess.run(['screencapture','-x',str(output/f'native-{scene}.png')],capture_output=True,timeout=10)
            assert capture.returncode==0,capture.stderr.decode(errors='replace')
            launches.append({'scene':scene,'nativeProcessSurvivedEightSeconds':True,'screenshotCaptured':True})
        process.terminate()
        try:process.wait(timeout=5)
        except subprocess.TimeoutExpired:process.kill();process.wait(timeout=5)
        process=None
    report={'version':version,'hostArchitecture':platform.machine(),'binaryArchitectures':arch.split(),'adHocSignatureVerified':True,'notarized':False,'dmgVerified':True,'localNetworkPurposePresent':True,'localMediaAllowed':True,'launches':launches,'deviceConnectionTested':False,'packageSha256':hashlib.sha256(target.read_bytes()).hexdigest()}
    (output/'package-check.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
finally:
    if process is not None and process.poll() is None:
        process.terminate()
        try:process.wait(timeout=5)
        except subprocess.TimeoutExpired:process.kill();process.wait(timeout=5)
    run('hdiutil','detach',str(mount))
