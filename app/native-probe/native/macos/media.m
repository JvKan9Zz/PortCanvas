// File-backed system decoders; every exported entry owns its autorelease pool.
#import <AVFoundation/AVFoundation.h>
#import <AudioToolbox/AudioToolbox.h>
#import <CoreVideo/CoreVideo.h>
#include <math.h>
#include <unistd.h>
static void pc_error(char *error, const char *message) {if(error) snprintf(error,1024,"%s",message ?: "macOS media error");}
@interface PCReader : NSObject
@property AVURLAsset *asset;
@property AVAssetReader *reader;
@property AVAssetReaderOutput *output;
@property BOOL audio;
@end
@implementation PCReader
- (BOOL)reset:(char *)error {
    [self.reader cancelReading];NSError *failure=nil;
    self.reader=[[AVAssetReader alloc] initWithAsset:self.asset error:&failure];
    NSArray *tracks=[self.asset tracksWithMediaType:AVMediaTypeVideo];
    if(!self.reader || !tracks.count){pc_error(error,failure.localizedDescription.UTF8String ?: "No video track");return NO;}
    AVAssetTrack *track=tracks.firstObject;
    AVAssetReaderVideoCompositionOutput *output=[[AVAssetReaderVideoCompositionOutput alloc] initWithVideoTracks:@[track] videoSettings:@{(id)kCVPixelBufferPixelFormatTypeKey:@(kCVPixelFormatType_32BGRA)}];
    output.videoComposition=[AVVideoComposition videoCompositionWithPropertiesOfAsset:self.asset];
    output.alwaysCopiesSampleData=NO;
    if(![self.reader canAddOutput:output]){pc_error(error,"Unsupported video format");return NO;}
    [self.reader addOutput:output];self.output=output;
    if(![self.reader startReading]){pc_error(error,self.reader.error.localizedDescription.UTF8String);return NO;}return YES;
}
@end
void *pc_video_open(const char *path,char *error) {@autoreleasepool {
    NSString *name=[[NSFileManager defaultManager] stringWithFileSystemRepresentation:path length:strlen(path)];
    PCReader *state=[PCReader new];state.asset=[AVURLAsset URLAssetWithURL:[NSURL fileURLWithPath:name] options:nil];
    state.audio=[state.asset tracksWithMediaType:AVMediaTypeAudio].count>0;
    if(![state reset:error])return NULL;return (__bridge_retained void*)state;
}}
int pc_video_audio(void *value){@autoreleasepool {return ((__bridge PCReader*)value).audio;}}
int pc_video_rewind(void *value,char *error){@autoreleasepool {return [(__bridge PCReader*)value reset:error];}}
int pc_video_next(void *value,uint8_t **pixels,uint32_t *width,uint32_t *height,int64_t *timestamp,int64_t *duration,char *error){@autoreleasepool {
    PCReader *state=(__bridge PCReader*)value;CMSampleBufferRef sample=[state.output copyNextSampleBuffer];
    if(!sample){if(state.reader.status==AVAssetReaderStatusFailed){pc_error(error,state.reader.error.localizedDescription.UTF8String);return -1;}return 0;}
    CVPixelBufferRef buffer=CMSampleBufferGetImageBuffer(sample);
    size_t w=CVPixelBufferGetWidth(buffer),h=CVPixelBufferGetHeight(buffer);
    if(!w||!h||w>16384||h>16384||w*h>67108864){CFRelease(sample);pc_error(error,"Video dimensions exceed native buffer limit");return -1;}
    if(CVPixelBufferLockBaseAddress(buffer,kCVPixelBufferLock_ReadOnly)!=kCVReturnSuccess){CFRelease(sample);pc_error(error,"Could not lock video frame");return -1;}
    size_t stride=CVPixelBufferGetBytesPerRow(buffer);uint8_t *source=CVPixelBufferGetBaseAddress(buffer),*out=malloc(w*h*4);
    if(!out||!source||stride<w*4){free(out);CVPixelBufferUnlockBaseAddress(buffer,kCVPixelBufferLock_ReadOnly);CFRelease(sample);pc_error(error,"Invalid video frame buffer");return -1;}
    for(size_t y=0;y<h;y++){const uint8_t *row=source+y*stride;uint8_t *dst=out+y*w*4;for(size_t x=0;x<w;x++){dst[4*x]=row[4*x+2];dst[4*x+1]=row[4*x+1];dst[4*x+2]=row[4*x];dst[4*x+3]=255;}}
    *pixels=out;*width=(uint32_t)w;*height=(uint32_t)h;
    CMTime time=CMSampleBufferGetPresentationTimeStamp(sample),length=CMSampleBufferGetDuration(sample);
    *timestamp=CMTIME_IS_NUMERIC(time)?CMTimeConvertScale(time,10000000,kCMTimeRoundingMethod_Default).value:0;
    *duration=CMTIME_IS_NUMERIC(length)&&CMTimeGetSeconds(length)>0?CMTimeConvertScale(length,10000000,kCMTimeRoundingMethod_Default).value:-1;
    CVPixelBufferUnlockBaseAddress(buffer,kCVPixelBufferLock_ReadOnly);CFRelease(sample);return 1;
}}
void pc_video_close(void *value){@autoreleasepool {PCReader *state=CFBridgingRelease(value);[state.reader cancelReading];}}
void *pc_audio_open(const char *path,int64_t position,float volume,char *error){@autoreleasepool {
    NSString *name=[[NSFileManager defaultManager] stringWithFileSystemRepresentation:path length:strlen(path)];NSError *failure=nil;
    AVAudioPlayer *player=[[AVAudioPlayer alloc] initWithContentsOfURL:[NSURL fileURLWithPath:name] error:&failure];
    if(!player){pc_error(error,failure.localizedDescription.UTF8String);return NULL;}
    player.volume=volume;player.currentTime=(double)position/10000000.;
    if(![player prepareToPlay]||![player play]){pc_error(error,"Audio output could not start");return NULL;}return (__bridge_retained void*)player;
}}
void pc_audio_close(void *value){@autoreleasepool {AVAudioPlayer *p=CFBridgingRelease(value);[p stop];}}
int64_t pc_audio_time(void *value){@autoreleasepool {return (int64_t)(((__bridge AVAudioPlayer*)value).currentTime*10000000.);}}
int pc_audio_ended(void *value){@autoreleasepool {AVAudioPlayer *p=(__bridge AVAudioPlayer*)value;return !p.playing && (p.currentTime<=0.0001 || p.currentTime>=p.duration-0.01);}}
void pc_audio_pause(void *value){@autoreleasepool {[(__bridge AVAudioPlayer*)value pause];}}
int pc_audio_play(void *value){@autoreleasepool {return [(__bridge AVAudioPlayer*)value play];}}
int pc_audio_seek(void *value,int64_t position){@autoreleasepool {AVAudioPlayer *p=(__bridge AVAudioPlayer*)value;p.currentTime=(double)position/10000000.;return [p play];}}
void pc_audio_volume(void *value,float volume){@autoreleasepool {((__bridge AVAudioPlayer*)value).volume=volume;}}
float pc_audio_get_volume(void *value){@autoreleasepool {return ((__bridge AVAudioPlayer*)value).volume;}}
// Synthetic H.264/AAC fixture, written only to a caller-owned nonexisting path.
int pc_media_fixture(const char *path,int audio,char *error){@autoreleasepool {
    NSString *name=[[NSFileManager defaultManager] stringWithFileSystemRepresentation:path length:strlen(path)];NSError *failure=nil;
    AVAssetWriter *writer=[[AVAssetWriter alloc] initWithURL:[NSURL fileURLWithPath:name] fileType:AVFileTypeMPEG4 error:&failure];
    if(!writer){pc_error(error,failure.localizedDescription.UTF8String);return 0;}
    AVAssetWriterInput *video=[AVAssetWriterInput assetWriterInputWithMediaType:AVMediaTypeVideo outputSettings:@{AVVideoCodecKey:AVVideoCodecTypeH264,AVVideoWidthKey:@320,AVVideoHeightKey:@180}];
    AVAssetWriterInputPixelBufferAdaptor *adaptor=[AVAssetWriterInputPixelBufferAdaptor assetWriterInputPixelBufferAdaptorWithAssetWriterInput:video sourcePixelBufferAttributes:@{(id)kCVPixelBufferPixelFormatTypeKey:@(kCVPixelFormatType_32BGRA),(id)kCVPixelBufferWidthKey:@320,(id)kCVPixelBufferHeightKey:@180}];
    [writer addInput:video];AVAssetWriterInput *sound=nil;
    if(audio){sound=[AVAssetWriterInput assetWriterInputWithMediaType:AVMediaTypeAudio outputSettings:@{AVFormatIDKey:@(kAudioFormatMPEG4AAC),AVSampleRateKey:@48000,AVNumberOfChannelsKey:@1,AVEncoderBitRateKey:@64000}];[writer addInput:sound];}
    if(![writer startWriting]){pc_error(error,writer.error.localizedDescription.UTF8String);return 0;}[writer startSessionAtSourceTime:kCMTimeZero];
    for(int n=0;n<30;n++){
        int wait=0;while(!video.readyForMoreMediaData&&wait++<1000)usleep(1000);
        CVPixelBufferRef buffer=NULL;
        if(!video.readyForMoreMediaData||CVPixelBufferPoolCreatePixelBuffer(NULL,adaptor.pixelBufferPool,&buffer)!=kCVReturnSuccess){[writer cancelWriting];pc_error(error,"Fixture video buffer failed");return 0;}
        CVPixelBufferLockBaseAddress(buffer,0);size_t stride=CVPixelBufferGetBytesPerRow(buffer);uint8_t *bytes=CVPixelBufferGetBaseAddress(buffer);
        for(int y=0;y<180;y++)for(int x=0;x<320;x++){uint8_t *p=bytes+y*stride+x*4;p[0]=(uint8_t)(x+n);p[1]=(uint8_t)y;p[2]=180;p[3]=255;}
        CVPixelBufferUnlockBaseAddress(buffer,0);BOOL ok=[adaptor appendPixelBuffer:buffer withPresentationTime:CMTimeMake(n,30)];CVPixelBufferRelease(buffer);
        if(!ok){[writer cancelWriting];pc_error(error,writer.error.localizedDescription.UTF8String);return 0;}
    }[video markAsFinished];
    if(sound){
        AudioStreamBasicDescription asbd={.mSampleRate=48000,.mFormatID=kAudioFormatLinearPCM,.mFormatFlags=kAudioFormatFlagIsSignedInteger|kAudioFormatFlagIsPacked,.mBytesPerPacket=2,.mFramesPerPacket=1,.mBytesPerFrame=2,.mChannelsPerFrame=1,.mBitsPerChannel=16};
        CMAudioFormatDescriptionRef format=NULL;CMAudioFormatDescriptionCreate(NULL,&asbd,0,NULL,0,NULL,NULL,&format);
        for(int n=0;n<10;n++){
            int wait=0;while(!sound.readyForMoreMediaData&&wait++<1000)usleep(1000);
            CMBlockBufferRef block=NULL;CMBlockBufferCreateWithMemoryBlock(NULL,NULL,9600,NULL,NULL,0,9600,0,&block);
            int16_t wave[4800];for(int i=0;i<4800;i++)wave[i]=(int16_t)(sin(2*M_PI*440*(n*4800+i)/48000.)*2000);
            CMBlockBufferReplaceDataBytes(wave,block,0,9600);CMSampleBufferRef sample=NULL;
            CMSampleTimingInfo timing={CMTimeMake(1,48000),CMTimeMake(n*4800,48000),kCMTimeInvalid};size_t size=2;
            OSStatus status=CMSampleBufferCreateReady(NULL,block,format,4800,1,&timing,1,&size,&sample);
            BOOL ok=status==noErr&&sound.readyForMoreMediaData&&[sound appendSampleBuffer:sample];if(sample)CFRelease(sample);CFRelease(block);
            if(!ok){CFRelease(format);[writer cancelWriting];pc_error(error,"Fixture audio encoding failed");return 0;}
        }CFRelease(format);[sound markAsFinished];
    }
    dispatch_semaphore_t done=dispatch_semaphore_create(0);[writer finishWritingWithCompletionHandler:^{dispatch_semaphore_signal(done);}];
    if(dispatch_semaphore_wait(done,dispatch_time(DISPATCH_TIME_NOW,10*NSEC_PER_SEC))!=0){[writer cancelWriting];pc_error(error,"Fixture writer timed out");return 0;}
    if(writer.status!=AVAssetWriterStatusCompleted){pc_error(error,writer.error.localizedDescription.UTF8String);return 0;}return 1;
}}
