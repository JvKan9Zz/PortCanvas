// Non-production contract harness; includes the bridge to inspect native ownership and order.
#import "window.m"
@interface PCContractDelegate : NSObject <NSApplicationDelegate>
@end
@implementation PCContractDelegate
@end
static int failed=0,last_action=0;
static void check(BOOL condition,const char *message){if(!condition){fprintf(stderr,"FAIL: %s\n",message);failed++;}}
static void action(void *context,int value){last_action=value;}
int main(void){@autoreleasepool {
    [NSApplication sharedApplication];PCContractDelegate *delegate=[PCContractDelegate new];NSApp.delegate=delegate;
    NSWindow *window=[[NSWindow alloc]initWithContentRect:NSMakeRect(100,100,1000,700) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable|NSWindowStyleMaskResizable backing:NSBackingStoreBuffered defer:NO];
    NSView *original=window.contentView;void *handle=pc_window_open((__bridge void*)original,NULL,action);
    check(handle!=NULL,"bridge created");if(!handle)return 1;
    PCChrome *state=(__bridge PCChrome*)handle;uint8_t pixels[16]={255,0,0,255,0,255,0,255,0,0,255,255,255,255,255,255};float crop[4]={0,0,1,1};
    pc_window_background(handle,pixels,2,2,crop);
    // Six values per surface: geometry and its modal flag. Modals ignore workspace opacity.
    float rects[12]={10,10,100,100,12,0,120,20,200,150,20,1};
    check(pc_window_material(handle,rects,2,1,.25,0,1)==1,"fallback native material is available");
    NSView *background=objc_getAssociatedObject(state,&pc_background_key);
    check(state.host.subviews.firstObject==background,"media stays below native material surfaces");
    check(window.contentView==original,"winit content view identity preserved");
    check(fabs(state.panels[0].alphaValue-.25)<.001,"normal surface opacity follows user setting");
    check(fabs(state.panels[1].alphaValue-1)<.001,"modal surface keeps thick opacity");
    check([NSApp.delegate respondsToSelector:@selector(applicationShouldTerminate:)],"Dock termination is intercepted");
    if([NSApp.delegate respondsToSelector:@selector(applicationShouldTerminate:)]){
        check([(id<NSApplicationDelegate>)NSApp.delegate applicationShouldTerminate:NSApp]==NSTerminateCancel,"Dock waits for shared quit decision");
        check(last_action==2,"Dock quit reaches Rust cleanup route");
    }
    BOOL edit=NO;for(NSMenuItem *item in state.menu.itemArray){if([item.submenu.title isEqualToString:@"Edit"])edit=YES;}
    check(edit,"native editing menu exposes Mac clipboard shortcuts");
    pc_window_background(handle,NULL,0,0,crop);check(background.layer.contents==nil,"cleared image releases native content");
    pc_window_close(handle);check(window.contentView==original,"teardown preserves renderer view");
    if(!failed)puts("Native AppKit window contracts passed.");return failed?1:0;
}}
