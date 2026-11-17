// AppKit owns system effects; the renderer stays above noninteractive material views.
#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>
#import <objc/runtime.h>
@class PCChrome;
static __weak PCChrome *pc_main_chrome;
static __weak NSWindow *pc_main_window;
static BOOL pc_reopen(id self,SEL selector,NSApplication *app,BOOL visible){
    if(!visible&&pc_main_window){[pc_main_window makeKeyAndOrderFront:nil];[app activateIgnoringOtherApps:YES];}return YES;
}
@interface PCMaterialHost : NSView
@end
@implementation PCMaterialHost
- (BOOL)isFlipped{return YES;}
@end
@interface PCVisualMaterial : NSVisualEffectView
@end
@implementation PCVisualMaterial
- (NSView*)hitTest:(NSPoint)point{return nil;}
@end
// A direct subclass of a weak-linked macOS 26 class cannot load on macOS 13.
// Create the noninteractive glass subclass only after runtime availability is known.
static NSView *pc_material_hit(id self,SEL selector,NSPoint point){return nil;}
static Class pc_glass_class(void){
    Class base=NSClassFromString(@"NSGlassEffectView");if(!base)return Nil;
    Class type=NSClassFromString(@"PCGlassMaterial");
    if(!type){type=objc_allocateClassPair(base,"PCGlassMaterial",0);class_addMethod(type,@selector(hitTest:),(IMP)pc_material_hit,"@@:{CGPoint=dd}");objc_registerClassPair(type);}return type;
}
@interface PCChrome : NSObject
@property NSWindow *window;
@property NSView *renderer;
@property PCMaterialHost *host;
@property NSMutableArray<NSView*> *panels;
@property NSInteger quality;
@property NSMenu *menu;
@property NSMenu *windowMenu;
@property id launched;
@property void *context;
@property void (*callback)(void*,int);
@end
@implementation PCChrome
- (void)dealloc{if(self.launched)[NSNotificationCenter.defaultCenter removeObserver:self.launched];}
- (void)settings:(id)sender{if(self.callback)self.callback(self.context,1);}
- (void)quit:(id)sender{if(self.callback)self.callback(self.context,2);}
- (void)close:(id)sender{[self.window performClose:nil];}
- (void)copy:(id)sender{if(self.callback)self.callback(self.context,3);}
- (void)paste:(id)sender{if(self.callback)self.callback(self.context,4);}
- (void)cut:(id)sender{if(self.callback)self.callback(self.context,5);}
- (void)selectAll:(id)sender{if(self.callback)self.callback(self.context,6);}
@end
// Dock Quit and termination requests take the same cancellable Rust shutdown path.
static NSApplicationTerminateReply pc_should_terminate(id self,SEL selector,NSApplication *app){
    PCChrome *chrome=pc_main_chrome;if(chrome.callback)chrome.callback(chrome.context,2);
    return NSTerminateCancel;
}
static void pc_add(NSMenu *menu,NSString *title,SEL action,NSString *key,id target){NSMenuItem *item=[[NSMenuItem alloc]initWithTitle:title action:action keyEquivalent:key];item.target=target;[menu addItem:item];}
void *pc_window_open(void *raw,void *context,void (*callback)(void*,int)) {@autoreleasepool {
    if(![NSThread isMainThread])return NULL;
    NSView *renderer=(__bridge NSView*)raw;NSWindow *window=renderer.window;if(!window)return NULL;
    PCChrome *state=[PCChrome new];state.window=window;state.renderer=renderer;state.context=context;state.callback=callback;state.panels=[NSMutableArray new];state.quality=-1;
    NSView *content=window.contentView;
    PCMaterialHost *host=[[PCMaterialHost alloc]initWithFrame:content.frame];host.autoresizingMask=NSViewWidthSizable|NSViewHeightSizable;
    // winit's NSWindowDelegate assumes contentView is its own WinitView subclass.
    // Preserve that identity; the materials are a sibling below the transparent renderer.
    if(!content.superview)return NULL;
    [content.superview addSubview:host positioned:NSWindowBelow relativeTo:content];state.host=host;
    NSCAssert(window.contentView==content,@"System materials must preserve the winit content view");
    window.opaque=NO;window.backgroundColor=NSColor.clearColor;window.titlebarAppearsTransparent=YES;pc_main_window=window;pc_main_chrome=state;
    if(NSApp.delegate&&![(id)NSApp.delegate respondsToSelector:@selector(applicationShouldHandleReopen:hasVisibleWindows:)])class_addMethod(object_getClass(NSApp.delegate),@selector(applicationShouldHandleReopen:hasVisibleWindows:),(IMP)pc_reopen,"B@:@B");
    if(NSApp.delegate&&![(id)NSApp.delegate respondsToSelector:@selector(applicationShouldTerminate:)])class_addMethod(object_getClass(NSApp.delegate),@selector(applicationShouldTerminate:),(IMP)pc_should_terminate,"q@:@");
    NSMenu *bar=[NSMenu new],*app=[NSMenu new];NSMenuItem *appItem=[NSMenuItem new];appItem.submenu=app;[bar addItem:appItem];
    pc_add(app,@"About PortCanvas",@selector(orderFrontStandardAboutPanel:),@"",NSApp);
    [app addItem:NSMenuItem.separatorItem];pc_add(app,@"Settings…",@selector(settings:),@",",state);
    [app addItem:NSMenuItem.separatorItem];pc_add(app,@"Hide PortCanvas",@selector(hide:),@"h",NSApp);pc_add(app,@"Show All",@selector(unhideAllApplications:),@"",NSApp);
    [app addItem:NSMenuItem.separatorItem];pc_add(app,@"Quit PortCanvas",@selector(quit:),@"q",state);
    NSMenuItem *editItem=[NSMenuItem new];NSMenu *edit=[[NSMenu alloc]initWithTitle:@"Edit"];editItem.submenu=edit;[bar addItem:editItem];
    pc_add(edit,@"Cut",@selector(cut:),@"x",state);pc_add(edit,@"Copy",@selector(copy:),@"c",state);pc_add(edit,@"Paste",@selector(paste:),@"v",state);pc_add(edit,@"Select All",@selector(selectAll:),@"a",state);
    NSMenuItem *windowItem=[NSMenuItem new];NSMenu *windowMenu=[[NSMenu alloc]initWithTitle:@"Window"];windowItem.submenu=windowMenu;[bar addItem:windowItem];
    pc_add(windowMenu,@"Minimize",@selector(performMiniaturize:),@"m",window);pc_add(windowMenu,@"Close Window",@selector(close:),@"w",state);pc_add(windowMenu,@"Zoom",@selector(performZoom:),@"",window);
    state.menu=bar;state.windowMenu=windowMenu;
    __weak PCChrome *weak=state;
    state.launched=[NSNotificationCenter.defaultCenter addObserverForName:NSApplicationDidFinishLaunchingNotification object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification *note){PCChrome *chrome=weak;if(chrome){NSApp.mainMenu=chrome.menu;NSApp.windowsMenu=chrome.windowMenu;}}];
    NSApp.mainMenu=bar;NSApp.windowsMenu=windowMenu;
    if(getenv("PORTCANVAS_WORKBENCH_REPORT")) {
        NSLog(@"PortCanvas native geometry window=%@ content=%@ screen=%@ visible=%@",NSStringFromRect(window.frame),NSStringFromRect(content.frame),NSStringFromRect(window.screen.frame),NSStringFromRect(window.screen.visibleFrame));
    }
    return (__bridge_retained void*)state;
}}
// Each panel is x,y,width,height,radius,modal in logical (point) coordinates.
int pc_window_material(void *value,const float *rects,size_t count,int quality,float opacity,int dark,int enabled){@autoreleasepool {
    if(![NSThread isMainThread]||!value)return 0;PCChrome *state=(__bridge PCChrome*)value;
    if(NSApp.mainMenu!=state.menu){NSApp.mainMenu=state.menu;NSApp.windowsMenu=state.windowMenu;}
    int style=0;if(enabled&&quality>0){if(@available(macOS 26.0,*)){style=quality==2?2:1;}else{style=1;}}
    if(state.quality!=style){for(NSView *view in state.panels)[view removeFromSuperview];[state.panels removeAllObjects];state.quality=style;}
    if(!style)return 0;
    while(state.panels.count>count){[state.panels.lastObject removeFromSuperview];[state.panels removeLastObject];}
    while(state.panels.count<count){NSView *view;
        if(@available(macOS 26.0,*)){if(style==2){NSGlassEffectView *glass=[pc_glass_class() new];glass.style=NSGlassEffectViewStyleRegular;glass.contentView=[NSView new];view=glass;}else{view=[PCVisualMaterial new];}}
        else{view=[PCVisualMaterial new];}
        if([view isKindOfClass:NSVisualEffectView.class]){NSVisualEffectView *effect=(NSVisualEffectView*)view;effect.material=NSVisualEffectMaterialSidebar;effect.blendingMode=NSVisualEffectBlendingModeBehindWindow;effect.state=NSVisualEffectStateFollowsWindowActiveState;}
        [state.host addSubview:view positioned:NSWindowAbove relativeTo:nil];[state.panels addObject:view];
    }
    for(NSUInteger n=0;n<count;n++){
        const float *r=rects+n*6;NSView *view=state.panels[n];view.frame=NSMakeRect(r[0],r[1],MAX(0,r[2]),MAX(0,r[3]));
        view.appearance=[NSAppearance appearanceNamed:dark?NSAppearanceNameDarkAqua:NSAppearanceNameAqua];view.alphaValue=r[5]>0?1:MAX(0,MIN(1,opacity));
        if(@available(macOS 26.0,*)){if([view isKindOfClass:NSGlassEffectView.class]){((NSGlassEffectView*)view).cornerRadius=r[4];continue;}}
        view.wantsLayer=YES;view.layer.cornerRadius=r[4];view.layer.masksToBounds=YES;
    }return style;
}}
void pc_window_show(void *value){@autoreleasepool {if(value){NSWindow *window=((__bridge PCChrome*)value).window;[NSApp unhide:nil];if(window.miniaturized)[window deminiaturize:nil];[window makeKeyAndOrderFront:nil];[NSApp activateIgnoringOtherApps:YES];}}}
void pc_window_hide(void *value){@autoreleasepool {if(value)[((__bridge PCChrome*)value).window orderOut:nil];}}
void pc_window_close(void *value){@autoreleasepool {PCChrome *state=CFBridgingRelease(value);for(NSView *view in state.panels)[view removeFromSuperview];[state.host removeFromSuperview];state.callback=NULL;state.context=NULL;}}
int pc_window_cursor(void *raw,float *x,float *y){@autoreleasepool {
    if(![NSThread isMainThread])return 0;NSView *view=(__bridge NSView*)raw;if(!view.window)return 0;
    NSPoint point=[view convertPoint:view.window.mouseLocationOutsideOfEventStream fromView:nil];
    *x=point.x;*y=view.isFlipped?point.y:view.bounds.size.height-point.y;return 1;
}}
unsigned pc_effects_read(void){@autoreleasepool {
    NSWorkspace *ws=NSWorkspace.sharedWorkspace;unsigned flags=0;
    if(ws.accessibilityDisplayShouldIncreaseContrast)flags|=1;
    if(ws.accessibilityDisplayShouldReduceTransparency)flags|=2;
    if(ws.accessibilityDisplayShouldReduceMotion)flags|=4;
    if(NSProcessInfo.processInfo.lowPowerModeEnabled)flags|=8;
    return flags;
}}
@interface PCObservers : NSObject
@property NSMutableArray *tokens;
@property void *context;
@property void (*callback)(void*);
@end
@implementation PCObservers
- (void)dealloc{for(NSArray *entry in self.tokens)[(NSNotificationCenter*)entry[0] removeObserver:entry[1]];}
@end
void *pc_effects_open(void *context,void(*callback)(void*)){@autoreleasepool {
    PCObservers *state=[PCObservers new];state.tokens=[NSMutableArray new];state.context=context;state.callback=callback;
    for(NSArray *entry in @[@[NSWorkspace.sharedWorkspace.notificationCenter,NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification],@[NSNotificationCenter.defaultCenter,NSProcessInfoPowerStateDidChangeNotification]]){
        __weak PCObservers *weak=state;id token=[(NSNotificationCenter*)entry[0] addObserverForName:entry[1] object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification *note){PCObservers *strong=weak;if(strong.callback)strong.callback(strong.context);}];[state.tokens addObject:@[entry[0],token]];
    }return (__bridge_retained void*)state;
}}
void pc_effects_close(void *value){@autoreleasepool {PCObservers *state=CFBridgingRelease(value);state.callback=NULL;state.context=NULL;}}
// CoreAnimation shares the same latest decoded frame as the ordinary Slint fallback.
// The layer is below system glass, so AppKit samples actual in-window media.
static char pc_background_key;
static void pc_release_pixels(void *info,const void *data,size_t length){free((void*)data);}
void pc_window_background(void *value,const uint8_t *pixels,uint32_t width,uint32_t height,const float *crop){@autoreleasepool {
    if(!value||![NSThread isMainThread])return;PCChrome *state=(__bridge PCChrome*)value;
    NSView *background=objc_getAssociatedObject(state,&pc_background_key);
    if(!background){background=[[NSView alloc]initWithFrame:state.host.bounds];background.autoresizingMask=NSViewWidthSizable|NSViewHeightSizable;background.wantsLayer=YES;[state.host addSubview:background positioned:NSWindowBelow relativeTo:nil];objc_setAssociatedObject(state,&pc_background_key,background,OBJC_ASSOCIATION_RETAIN_NONATOMIC);}
    if(!pixels||!width||!height){background.layer.contents=nil;return;}
    size_t length=(size_t)width*height*4;uint8_t *copy=malloc(length);if(!copy)return;memcpy(copy,pixels,length);
    CGDataProviderRef provider=CGDataProviderCreateWithData(NULL,copy,length,pc_release_pixels);CGColorSpaceRef colors=CGColorSpaceCreateDeviceRGB();
    CGImageRef image=CGImageCreate(width,height,8,32,(size_t)width*4,colors,kCGImageAlphaLast|kCGBitmapByteOrderDefault,provider,NULL,false,kCGRenderingIntentDefault);
    CGImageRef clipped=image?CGImageCreateWithImageInRect(image,CGRectMake(crop[0]*width,crop[1]*height,MAX(1,crop[2]*width),MAX(1,crop[3]*height))):NULL;
    [CATransaction begin];[CATransaction setDisableActions:YES];background.layer.contents=(__bridge id)clipped;background.layer.contentsGravity=kCAGravityResizeAspectFill;[CATransaction commit];
    if(clipped)CGImageRelease(clipped);if(image)CGImageRelease(image);CGColorSpaceRelease(colors);CGDataProviderRelease(provider);
}}
void pc_window_color(void *value,float red,float green,float blue){@autoreleasepool {
    if(!value||![NSThread isMainThread])return;PCChrome *state=(__bridge PCChrome*)value;
    state.host.wantsLayer=YES;state.host.layer.backgroundColor=[NSColor colorWithSRGBRed:red green:green blue:blue alpha:1].CGColor;
}}
