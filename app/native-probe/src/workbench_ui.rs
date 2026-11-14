slint::slint! {
    export {ActivityPattern} from "action_button.slint";
    import {CheckBox} from "check_box.slint";
    import {SelectBox as ComboBox} from "select_box.slint";
    import {ScrollView,ListView,TextEdit,VerticalBox,HorizontalBox,Palette,AboutSlint} from "std-widgets.slint";
    import {FormInput as LineEdit} from "form_input.slint";
    import {TerminalView,TerminalCell,TerminalTab,CompletionRow} from "terminal_view.slint";
    import {DesktopTool} from "desktop_tool.slint";
    import {SettingsTab,SegmentedChoice,EnvironmentOption} from "settings_controls.slint";
    import {QuickConnect} from "quick_connect.slint";
    import {ColorPicker} from "color_picker.slint";
    import {DesktopView} from "desktop_view.slint";
    import {GlassSettings} from "glass_view.slint";
    import {BackgroundSettings,BackgroundCropView} from "background_view.slint";
    import {ArenaView,MetricRow} from "arena_view.slint";
    import {FilesView,FileRow,FileArea} from "files_view.slint";
    import {ActionButton,ActionButton as Button,AppColors} from "action_button.slint";
    import {MaterialPanel,NavButton,DeviceListItem,ConnectionBadge} from "workbench_chrome.slint";
    export struct ChromeLayout {nav:float,drawer:float,drawer-x:float,settings-y:float,settings-opacity:float,settings-tab:float,page:float,dialog-y:float,dialog-opacity:float,advanced:float,connection-height:float,busy-phase:float,busy-wave:float,busy:float}
    export struct MaterialRect {x:float,y:float,width:float,height:float,radius:float}
    import {LogsView,LogRow,LogFact} from "logs_view.slint";
    import {DevicesView,DeviceRow} from "devices_view.slint";
    export component Workbench inherits Window {
        callback notify(string);in property <string> toast-message;in-out property <bool> toast-open:false;
        property <float> toast-phase:root.toast-open?1:0;
        animate toast-phase {duration:AppColors.motion?(root.toast-open?280ms:180ms):0ms;easing:cubic-bezier(0.16,1,0.3,1);}
        in-out property <bool> quick-open:false;in-out property <bool> quick-candidate:false;
        callback quick-submit();callback quick-cancel();callback quick-finished();
        property <float> quick-phase:root.quick-open?1:0;
        animate quick-phase {duration:AppColors.motion?(root.quick-open?260ms:180ms):0ms;easing:cubic-bezier(0.16,1,0.3,1);}
        changed quick-open=>{if root.quick-open{quick-view.focus-first();}}
        changed quick-phase=>{if root.quick-phase==0&&!root.quick-open{main-focus.focus();}}
        in property <string> app-version;
        title:"PortCanvas Beta · v"+root.app-version;
        icon:@image-url("../assets/portcanvas.png");
        preferred-width:1200px;preferred-height:820px;min-width:1000px;min-height:700px;
        default-font-family:root.english?"Segoe UI":"Microsoft YaHei UI";default-font-size:14px;
        background:Palette.background;
        no-frame:root.window-is-fullscreen;
        init=>{Palette.color-scheme=ColorScheme.light;}
        pure callback tr(string,bool)->string;
        // Native children may evaluate before Rust installs the translation handler.
        tr(source,english)=>{return source;}
        public function focus-main(){main-focus.focus();}
        in-out property <bool> english:false;
        in-out property <int> page:0;
        property <bool> page-entering:false;property <float> page-phase:root.page-entering&&AppColors.motion?0:1;
        animate page-phase {duration:root.page-entering||!AppColors.motion?0ms:280ms;easing:cubic-bezier(0.16,1,0.3,1);}
        Timer {interval:AppColors.motion?16ms:1ms;running:root.page-entering;triggered=>{root.page-entering=false;}}
        in property <bool> observe-motion:false;
        property <float> observed-advanced;property <float> observed-connection;property <int> observed-busy-phase;property <int> observed-busy-wave;property <bool> observed-busy;
        in property <[DeviceRow]> devices;
        in property <[DeviceRow]> found;
        in-out property <string> selected-id;
        in-out property <string> device-name;
        in-out property <string> host;
        in-out property <string> port:"22";
        in-out property <string> username;
        in-out property <int> auth:0;
        in-out property <string> key-path;
        in-out property <string> password;
        in-out property <bool> remember:false;
        in-out property <bool> advanced:false;
        in property <string> settings-error;
        property <bool> third-party-open:false;
        in property <string> update-phase;in property <string> update-message;in property <string> update-version;in property <string> update-notes;
        in property <bool> update-busy;in property <float> update-progress;
        changed update-busy=>{root.material-controls();}
        callback update-action(string);callback update-changed();
        in property <bool> bg-video;
        in property <float> bg-picker-hue;in property <float> bg-picker-saturation;in property <float> bg-picker-brightness;
        in property <int> bg-picker-red;in property <int> bg-picker-green;in property <int> bg-picker-blue;
        in property <color> bg-picker-color;in property <color> bg-picker-hue-color;
        in-out property <string> bg-picker-hex;in property <string> bg-picker-error;
        callback bg-picker-change(float,float,float);callback bg-picker-rgb(int,int,int);callback bg-picker-hex-edited(string);callback bg-picker-response(bool);
        callback settings-backdrop(bool);callback settings-backdrop-ready(string,image,float);
        in property <image> settings-backdrop-frame;in property <bool> settings-backdrop-active;
        property <float> settings-backdrop-phase:root.settings-backdrop-active?1:0;
        animate settings-backdrop-phase {duration:AppColors.motion?120ms:0ms;easing:ease-out;}
        in-out property <bool> settings-open:false;
        in-out property <bool> settings-presented:false;
        in-out property <int> settings-section:0;
        property <float> settings-progress:root.settings-presented?1:0;
        animate settings-progress {duration:AppColors.motion?(root.settings-open?320ms:220ms):0ms;easing:cubic-bezier(0.22,0.61,0.36,1);}
        changed settings-progress=>{if root.settings-progress==0&&!root.settings-open{root.settings-backdrop(false);settings-entry.focus();}}
        property <bool> settings-entering:false;
        property <float> settings-content-phase:root.settings-entering&&AppColors.motion?0:1;
        animate settings-content-phase {duration:root.settings-entering||!AppColors.motion?0ms:220ms;easing:cubic-bezier(0.16,1,0.3,1);}
        Timer {interval:AppColors.motion?16ms:1ms;running:root.settings-entering;triggered=>{root.settings-entering=false;}}
        changed settings-section=>{root.settings-entering=true;if root.settings-section==1&&root.settings-open{root.refresh-logs();}}
        in property <string> log-usage;in property <bool> log-usage-known:false;
        changed settings-open=>{if root.settings-open{root.settings-backdrop(true);}else{root.settings-presented=false;}root.bg-sync();root.material-controls();root.terminal-cancel-completion();if root.settings-open&&root.settings-section==1{root.refresh-logs();}if root.settings-open{if root.settings-section==1{settings-storage.focus();}else if root.settings-section==2{settings-about.focus();}else{settings-general.focus();}}else if root.settings-progress==0{settings-entry.focus();}}
        in-out property <bool> connection-details-open:false;
        changed busy=>{if root.busy{root.connection-details-open=true;}}
        in-out property <string> display:":0";
        in-out property <string> xauthority;
        in property <bool> system-busy:false;in property <bool> system-executing:false;in property <string> system-error;callback system-action(string);callback system-confirm(string,string);callback system-finished(string,string);
        in property <bool> busy:false;
        in property <bool> connected:false;
        in property <bool> discovering:false;
        in property <string> discovery-progress;
        in property <string> connection-status:"尚未连接设备";
        in property <int> connection-stage:-1;
        in property <string> connection-detail;
        in property <string> error;changed error=>{if root.error!=""{root.connection-details-open=true;}}
        in property <string> data-directory;
        in-out property <int> appearance:0;
        in-out property <int> theme:0;
        in property <bool> window-is-fullscreen:false;
        in property <bool> native-hidden:false;
        in property <bool> native-focused:true;
        changed native-focused=>{root.material-controls();}
        in property <[FileRow]> file-local-rows;in property <[FileRow]> file-remote-rows;
        in property <[string]> file-drives;in-out property <int> file-drive-index:-1;callback files-drive(int);
        in-out property <FileArea> file-local-area;in-out property <FileArea> file-remote-area;
        in-out property <int> file-geometry-ticket;
        in property <int> file-drop-side:-1;in property <int> file-drop-row:-1;in property <string> file-drop-label;
        in-out property <string> file-local-path;in-out property <string> file-remote-path;
        in-out property <int> file-folder-side;in-out property <string> file-folder-name;
        in-out property <string> file-local-folder;in-out property <string> file-remote-folder;
        in property <string> file-local-message;in property <string> file-remote-message;
        in property <bool> file-local-loading;in property <bool> file-remote-loading;
        in property <bool> file-local-loaded;in property <bool> file-remote-loaded;
        in property <int> file-local-selected;in property <int> file-remote-selected;
        in property <bool> file-mutating;in property <string> file-status;in property <string> file-current;in property <string> file-numbers;in property <string> file-error;
        in property <float> file-progress;in-out property <int> file-rate:0;
        callback files-sync();callback files-state-changed();callback files-action(int,string);
        callback files-choose(int,int,bool,bool);callback files-all(int,bool);callback files-open(int,int);
        callback files-load-result(int,string,string,string);callback files-confirm(string,string,bool,string);callback files-finished(string,string);
        in property <[string]> arena-cameras;in-out property <int> arena-camera-index:0;in-out property <int> arena-rate:2;
        in property <bool> arena-camera-active;in property <bool> arena-camera-loading;in property <bool> arena-has-frame;in property <image> arena-frame;
        in property <string> arena-camera-status;in property <string> arena-metrics-status;
        in property <[MetricRow]> arena-metric-rows;
        in property <string> arena-cpu-path;in property <string> arena-memory-path;
        in property <bool> arena-pose-live;in property <float> arena-world-x;in property <float> arena-world-y;in property <float> arena-heading;in property <bool> arena-has-heading;
        in property <string> arena-pose-status;in property <string> arena-coordinates;
        callback arena-plot-changed(float);callback arena-sync();callback arena-toggle-camera();callback arena-refresh-cameras();callback arena-environment(string);
        callback arena-metrics-result(string,string);callback arena-cameras-result(string,string,string);callback arena-camera-finished(string,string);
        callback arena-pose-started(string,string);callback arena-pose-packet(string);
        in-out property <bool> terminal-open:false;
        out property <bool> terminal-visible:root.page!=0&&root.terminal-open&&!root.desktop-expanded;
        changed terminal-visible=>{root.terminal-cancel-completion();if root.terminal-visible{root.terminal-reload();}else if !root.desktop-expanded{main-focus.focus();}}
        in property <bool> material-active:false;
        in property <bool> material-solid:false;
        in property <bool> high-contrast:false;changed high-contrast=>{AppColors.high-contrast=root.high-contrast;}
        in property <int> material-suggestion-target:1;
        in property <string> material-status;
        callback material-refresh();callback material-controls();callback material-response(bool);
        changed material-solid=>{AppColors.motion=!root.material-solid&&!root.native-hidden;root.bg-sync();}
        in property <image> material-nav;in property <image> material-header;in property <image> material-terminal;in property <image> material-work;
        out property <color> material-tint:Palette.background;
        out property <MaterialRect> material-nav-rect:{x:16,y:16,width:84,height:(root.height - 32px)/1px,radius:24};
        out property <MaterialRect> material-header-rect:root.desktop-expanded?{x:(work-area.x+work-layout.padding-left)/1px,y:(work-area.y+work-layout.padding-top)/1px,width:(work-area.width - work-layout.padding-left - work-layout.padding-right)/1px,height:46,radius:14}:{x:116,y:16,width:(root.width - 132px)/1px,height:48,radius:17};
        out property <MaterialRect> desktop-work-rect:{x:work-area.x/1px,y:work-area.y/1px,width:work-area.width/1px,height:work-area.height/1px,radius:work-area.border-radius/1px};
        out property <MaterialRect> material-terminal-rect:{x:terminal-panel.x/1px,y:76,width:terminal-panel.width/1px,height:(root.height - 92px)/1px,radius:terminal-panel.border-radius/1px};
        out property <ChromeLayout> chrome-layout:{nav:nav-pill.y/1px,drawer:root.drawer-space/1px,drawer-x:terminal-panel.x/1px,settings-y:settings-panel.y/1px,settings-opacity:root.settings-progress,settings-tab:settings-pill.y/1px,page:root.page-phase,dialog-y:modal-panel.y/1px,dialog-opacity:root.dialog-phase,advanced:root.observed-advanced,connection-height:root.observed-connection,busy-phase:root.observed-busy-phase,busy-wave:root.observed-busy-wave,busy:root.observed-busy?1:0};
        property <int> nav-index:root.page==5?1:root.page==6?2:root.page==3?3:root.page==7?4:0;
        property <bool> compact-content:root.terminal-visible&&root.width<1360px;
        property <length> drawer-preferred:-1px;
        property <bool> drawer-resizing:false;
        property <length> drawer-maximum:max(320px,root.width - 692px);
        property <length> drawer-width:min(root.drawer-maximum,max(320px,root.drawer-preferred<0px?min(580px,max(360px,(root.width - 132px)*0.4)):root.drawer-preferred));
        property <length> drawer-space:root.page!=0&&root.terminal-open?root.drawer-width+14px:0px;
        animate drawer-space {duration:AppColors.motion&&!root.drawer-resizing?320ms:0ms;easing:cubic-bezier(0.16,1,0.3,1);}
        in property <image> bg-frame;in property <bool> bg-has-frame;in property <bool> bg-custom;in property <color> bg-color:#eaf0f8;
        in property <int> bg-width;in property <int> bg-height;in property <bool> bg-busy;in property <bool> bg-cancellable;
        in-out property <string> bg-hex:"#eaf0f8";in-out property <float> bg-volume:0;in-out property <float> bg-material:100;
        in-out property <int> glass-quality:2;changed glass-quality=>{root.material-controls();}in property <bool> glass-loaded:false;in property <string> glass-error;callback glass-controls(bool);
        in property <string> bg-status;in property <string> bg-error;
        in property <float> bg-saved-crop-x;in property <float> bg-saved-crop-y;in property <float> bg-saved-crop-width:1;in property <float> bg-saved-crop-height:1;
        in property <image> bg-preview;in property <int> bg-preview-width:1;in property <int> bg-preview-height:1;
        in property <float> bg-crop-x;in property <float> bg-crop-y;in property <float> bg-crop-width:1;in property <float> bg-crop-height:1;
        callback bg-sync();callback bg-action(string);callback bg-controls();callback bg-progress(string,string,string);callback bg-ready(string,string);
        callback bg-crop-response(bool);callback bg-crop-reset();callback bg-crop-gesture(int,float,float);callback bg-crop-arrow(float,float,bool);
        changed bg-busy=>{root.bg-sync();}
        in property <image> desktop-frame;
        in property <int> desktop-source-width;
        in property <int> desktop-source-height;
        in property <image> desktop-cursor;
        in property <bool> desktop-cursor-known;
        in property <int> desktop-cursor-width;
        in property <int> desktop-cursor-height;
        in property <int> desktop-cursor-hot-x;
        in property <int> desktop-cursor-hot-y;
        in property <bool> desktop-ready:false;
        in property <bool> desktop-expanded:false;
        callback desktop-expand(bool);
        in-out property <bool> desktop-tools-open:false;
        changed desktop-expanded=>{root.bg-sync();root.material-controls();}
        in property <bool> desktop-active:false;
        in property <bool> desktop-wanted:false;
        in property <bool> desktop-clipboard-available:false;
        in-out property <bool> desktop-readonly:false;
        in-out property <bool> desktop-fit:true;
        in property <string> desktop-status;
        in property <string> desktop-error;
        in-out property <int> desktop-focus-revision;
        callback desktop-view-changed();callback desktop-quality-changed();callback desktop-action(string);callback desktop-readonly-changed();
        callback desktop-pointer(float,float,float,float,int,bool);callback desktop-motion(float,float,float,float);callback desktop-wheel(float,float,float,float,int,int);
        callback desktop-key(string,bool,bool,bool)->bool;callback desktop-text(string);callback desktop-focused(bool);
        callback desktop-notice(string,string,string,string,bool);callback desktop-finished(string,string);callback desktop-drawn(string);callback desktop-environment(string);
        changed connected=>{if root.connected{root.connection-details-open=true;if root.quick-candidate{root.quick-finished();}}root.desktop-view-changed();}
        changed english=>{root.arena-sync();root.files-sync();root.desktop-view-changed();}
        in property <[TerminalTab]> terminal-tabs;
        in property <[TerminalCell]> terminal-cells;
        in property <string> terminal-active;
        in property <string> terminal-phase;
        in property <string> terminal-status;in property <string> terminal-information;
        in property <string> terminal-error;
        in property <string> terminal-log-status;
        in property <bool> terminal-busy:false;
        in property <bool> terminal-sending:false;
        in property <int> terminal-cursor-column;
        in property <int> terminal-cursor-row;
        in property <bool> terminal-cursor-visible;
        in-out property <string> terminal-draft;
        in-out property <string> terminal-project;
        in property <[CompletionRow]> terminal-completions;
        in property <int> terminal-completion-index;
        in property <bool> terminal-completion-loading;
        in property <bool> terminal-completion-truncated;
        in property <int> terminal-command-caret;
        in property <int> terminal-command-revision;
        out property <bool> terminal-completion-active:root.terminal-completion-loading||root.terminal-completions.length>0;
        callback terminal-reload();callback terminal-flush();callback terminal-failure(string,string,string);callback terminal-sent(string,string,string);
        callback terminal-close-tab(string);callback terminal-select(string);callback terminal-action(string);callback terminal-copy();callback terminal-paste();
        callback terminal-scroll(int);callback terminal-selection(int,int,int,int);callback terminal-resize(int,int);
        callback terminal-committed(string);callback terminal-key(string,bool,bool,bool)->bool;
        callback terminal-draft-edited(string,int);callback terminal-project-edited(string);callback terminal-send-command();
        callback terminal-editor-key(string,bool,bool,bool,bool)->bool;callback terminal-editor-focused(bool);
        callback terminal-choose-completion(int);callback terminal-cancel-completion();
        callback terminal-completion-result(string,string,string,string,string);
        in property <[LogRow]> log-rows;
        in property <[string]> log-projects;
        in property <[string]> log-days;
        in-out property <int> log-project-index:0;
        in-out property <int> log-day-index:0;
        in-out property <int> log-status-index:0;
        in property <bool> logs-loading:false;
        in property <bool> logs-mutating:false;
        in property <bool> logs-selectable:false;
        in-out property <bool> logs-all-checked:false;
        in property <int> logs-checked:0;
        in property <bool> logs-back:false;
        in property <bool> logs-next:false;
        in property <string> logs-summary;
        in property <string> logs-notice;
        in property <string> logs-message;
        in property <string> logs-error;
        in property <string> log-directory;
        in property <bool> log-selected:false;
        in property <bool> log-reading:false;
        in property <bool> log-live:false;
        in property <string> log-command;
        in property <string> log-metadata;
        in property <[LogFact]> log-left-facts;in property <[LogFact]> log-right-facts;in property <string> log-file;
        in property <string> log-warning;
        in property <string> log-output;
        property <bool> can-browse:!root.logs-loading&&!root.logs-mutating&&root.can-interact;
        callback refresh-logs();callback poll-logs();callback filter-logs(int,int,int);callback log-page(bool);
        callback select-log(string);callback toggle-log(string,bool);callback select-all-logs(bool);callback remove-logs();
        callback choose-log-directory();callback open-log-directory(bool);callback fullscreen();callback set-theme(int);
        changed theme=>{AppColors.preferred-accent=root.theme==1?#2b8169:root.theme==2?#53657f:#356ce5;}
        changed page=>{root.page-entering=true;if root.page!=5{root.desktop-expand(false);}root.bg-sync();root.arena-sync();root.files-sync();root.desktop-view-changed();root.terminal-cancel-completion();if root.page==3{root.refresh-logs();}if root.terminal-visible{root.terminal-reload();}}
        changed native-hidden=>{AppColors.motion=!root.material-solid&&!root.native-hidden;root.bg-sync();root.arena-sync();root.files-sync();root.desktop-view-changed();if root.native-hidden{root.terminal-cancel-completion();}if !root.native-hidden&&root.page==3{root.refresh-logs();}if !root.native-hidden&&root.terminal-visible{root.terminal-reload();}}
        Timer {interval:1200ms;running:root.page==3&&root.log-live&&!root.native-hidden&&root.can-interact;triggered=>{root.poll-logs();}}

        in-out property <int> environment-source:0;
        in-out property <int> desktop-mode:0;
        changed desktop-mode=>{root.desktop-quality-changed();}
        in-out property <string> log-limit:"1024";
        in-out property <int> dialog-kind:0;in-out property <int> painted-dialog:0;
        property <float> dialog-phase:root.dialog-kind!=0?1:0;
        animate dialog-phase {duration:AppColors.motion?(root.dialog-kind!=0?180ms:140ms):0ms;easing:cubic-bezier(0.16,1,0.3,1);}
        changed dialog-phase=>{if root.dialog-phase==0&&root.dialog-kind==0{let previous=root.painted-dialog;root.painted-dialog=0;if root.quick-open{quick-view.focus-first();}else if root.settings-open{if previous==14{root.color-focus-ticket+=1;}else if root.settings-section==1{settings-storage.focus();}else if root.settings-section==2{settings-about.focus();}else{settings-general.focus();}}else{main-focus.focus();}}}property <int> color-focus-ticket;property <length> color-anchor-x;property <length> color-anchor-y;
        property <bool> can-interact:root.painted-dialog==0&&!root.quick-open&&!root.system-busy&&!root.migration-busy;
        in property <bool> migration-busy:false;in property <bool> migration-executing:false;
        in property <string> migration-status;in property <string> migration-error;
        callback migration-action(bool);callback migration-confirm(string,string,string,bool);callback migration-finished(string,string,string);
        in property <string> dialog-title;
        in property <string> dialog-message;
        property <bool> accept-focused:confirm-response.focused;
        property <bool> can-edit:!root.busy && !root.connected && !root.logs-mutating && root.can-interact;
        callback select-device(string);callback select-found(string);callback new-device();
        callback save-device();callback connect-device();callback disconnect-device();callback remove-device();
        callback choose-key();callback discover();callback cancel-discovery();callback respond(bool);
        callback set-language(int);callback set-appearance(int);callback set-environment(int);callback set-desktop-mode(int);callback save-log-limit(string);
        changed appearance=>{Palette.color-scheme=root.appearance==0?ColorScheme.light:root.appearance==1?ColorScheme.dark:ColorScheme.unknown;}
        changed dialog-kind=>{if root.dialog-kind!=0{root.painted-dialog=root.dialog-kind;}root.bg-sync();if root.dialog-kind==14{picker-view.focus-selection();}else if root.dialog-kind==8{crop-view.focus-selection();}else if root.dialog-kind==15{folder-name.focus();}else if root.dialog-kind!=0 {cancel-response.focus();}else if root.settings-open{settings-general.focus();}else{main-focus.focus();}}
        Rectangle {background:root.bg-custom?root.bg-color:AppColors.dark?#131e30:@linear-gradient(135deg,#edf3fa,#f4f5fa 58%,#e9eff8);
            if root.bg-has-frame:Image {source:root.bg-frame;width:parent.width;height:parent.height;image-fit:cover;source-clip-x:round(root.bg-saved-crop-x*root.bg-width);source-clip-y:round(root.bg-saved-crop-y*root.bg-height);source-clip-width:max(1,round(root.bg-saved-crop-width*root.bg-width));source-clip-height:max(1,round(root.bg-saved-crop-height*root.bg-height));}
        }
        if root.material-solid:Rectangle {background:Palette.background;}
        // THESIS: A clear work area between the familiar left rail and optional shared terminal.
        // OWN-WORLD: User-pinned Liquid Glass; bounded background-only GPU surfaces and native controls.
        // STORY: Select a workspace, work in place, open the same terminal without leaving that page.
        // FIRST VIEWPORT: 88px left rail, 64px header, clear body; terminal 420-540px at the right.
        // FORM: Code-led extension of the approved PortCanvas desktop structure; no new identity choice.
        // FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, and DESIGN.md
        main-focus:=FocusScope {enabled:root.painted-dialog==0&&root.settings-progress==0&&root.quick-phase==0&&!root.system-busy&&!root.migration-busy;
            MaterialPanel {visible:!root.desktop-expanded;x:16px;y:16px;width:84px;height:parent.height - 32px;material:root.material-nav;active:root.material-active;strength:root.bg-material;solid:root.material-solid;
                VerticalBox {padding-left:9px;padding-right:9px;padding-top:20px;padding-bottom:12px;spacing:12px;
                    Rectangle {height:58px;vertical-stretch:0;background:transparent;Image {x:(parent.width - 43px)/2;width:43px;height:43px;source:@image-url("../assets/portcanvas.png");image-fit:contain;}}
                    Rectangle {height:352px;vertical-stretch:0;background:transparent;
                        nav-pill:=Rectangle {transform-origin:{x:self.width/2,y:self.height/2};transform-scale:root.nav-index==0?nav-devices.press-scale:root.nav-index==1?nav-desktop.press-scale:root.nav-index==2?nav-files.press-scale:root.nav-index==3?nav-logs.press-scale:nav-arena.press-scale;y:root.nav-index*72px;width:parent.width;height:64px;border-radius:17px;background:AppColors.soft;animate y {duration:AppColors.motion?320ms:0ms;easing:cubic-bezier(0.22,0.61,0.36,1);}}
                        VerticalBox {padding:0px;spacing:8px;
                            nav-devices:=NavButton {text:root.english?"Devices":"设备";icon:@image-url("icons/devices.svg");shared-selection:true;selected:root.page==0;enabled:root.can-interact;clicked=>{root.page=0;}}
                            nav-desktop:=NavButton {text:root.english?"Desktop":"桌面";icon:@image-url("icons/desktop.svg");shared-selection:true;selected:root.page==5;enabled:root.can-interact;clicked=>{root.page=5;}}
                            nav-files:=NavButton {text:root.english?"Files":"文件";icon:@image-url("icons/files.svg");shared-selection:true;selected:root.page==6;enabled:root.can-interact;clicked=>{root.page=6;}}
                            nav-logs:=NavButton {text:root.english?"Logs":"日志";icon:@image-url("icons/logs.svg");shared-selection:true;selected:root.page==3;enabled:root.can-interact;clicked=>{root.page=3;}}
                            nav-arena:=NavButton {text:root.english?"Arena":"赛台";icon:@image-url("icons/arena.svg");shared-selection:true;selected:root.page==7;enabled:root.can-interact;clicked=>{root.page=7;}}
                        }
                    }
                    Rectangle {vertical-stretch:1;background:transparent;}
                    settings-entry:=NavButton {text:root.english?"Settings":"设置";icon:@image-url("icons/settings.svg");selected:root.settings-open;enabled:root.can-interact;clicked=>{root.settings-open=true;}}
                }
            }
            MaterialPanel {visible:!root.desktop-expanded;x:116px;y:16px;width:parent.width - 132px;height:48px;border-radius:17px;material:root.material-header;active:root.material-active;strength:root.bg-material;solid:root.material-solid;
                HorizontalBox {padding-left:20px;padding-right:16px;spacing:12px;
                    Image {source:@image-url("../assets/wordmark.svg");image-fit:fill;width:156px;height:35px;horizontal-stretch:0;vertical-stretch:0;}
                    if root.update-phase=="available":ActionButton {text:"v"+root.update-version;enabled:root.can-interact;clicked=>{root.settings-section=2;root.settings-open=true;}}
                    Rectangle {background:transparent;horizontal-stretch:1;}
                    ConnectionBadge {text:root.tr(root.connection-status,root.english);phase:root.connected?2:root.busy?1:root.error!=""?3:0;}
                    if root.page!=0:Rectangle {width:96px;horizontal-stretch:0;background:transparent;
                        ActionButton {y:(parent.height - self.height)/2;text:root.english?"Terminal":"终端";icon:@image-url("icons/terminal.svg");has-icon:true;primary:root.terminal-open;width:parent.width;enabled:root.can-interact;accessible-checkable:true;accessible-checked:root.terminal-open;clicked=>{root.terminal-open=!root.terminal-open;}}
                    }
                }
            }
            work-area:=MaterialPanel {material:root.material-work;active:root.material-active&&!root.desktop-expanded;strength:root.bg-material;solid:root.material-solid;x:116px;y:76px;width:parent.width - 132px - root.drawer-space;height:parent.height - 92px;
                flat:root.desktop-expanded;border-radius:24px;
                states [expanded when root.desktop-expanded:{
                    x:12px;y:12px;width:parent.width - 24px;height:parent.height - 24px;border-radius:14px;
                    work-layout.padding-left:0px;work-layout.padding-right:0px;work-layout.padding-top:0px;work-layout.padding-bottom:0px;
                    in {animate x,y,width,height,border-radius {duration:root.material-solid?0ms:240ms;easing:ease-out;}animate work-layout.padding-left,work-layout.padding-right,work-layout.padding-top,work-layout.padding-bottom {duration:root.material-solid?0ms:240ms;easing:ease-out;}}
                    out {animate x,y,width,height,border-radius {duration:root.material-solid?0ms:240ms;easing:ease-out;}animate work-layout.padding-left,work-layout.padding-right,work-layout.padding-top,work-layout.padding-bottom {duration:root.material-solid?0ms:240ms;easing:ease-out;}}
                }]
                work-layout:=VerticalBox {y:(1-root.page-phase)*7px;padding-left:root.width<1120px?22px:30px;padding-right:root.width<1120px?22px:30px;padding-top:26px;padding-bottom:24px;spacing:20px;
            if !root.desktop-expanded&&root.system-error!="":Text {text:root.tr(root.system-error,root.english);color:#ce3535;wrap:word-wrap;vertical-stretch:0;}
            if !root.desktop-expanded&&root.page!=1&&root.bg-error!="":Text {text:root.tr(root.bg-error,root.english);color:#ce3535;wrap:word-wrap;vertical-stretch:0;}
            if !root.desktop-expanded&&root.migration-status!="":Text {text:root.tr(root.migration-status,root.english);color:Palette.foreground;wrap:word-wrap;vertical-stretch:0;}
            if !root.desktop-expanded&&root.migration-error!="":Text {text:root.tr(root.migration-error,root.english);color:#ce3535;wrap:word-wrap;vertical-stretch:0;}
            // DEVICE PATH: flat selectable list, scrollable credentials, always-reachable actions.
            // Preserve the existing profile, trust, cancellation and migration callbacks.
            if !root.desktop-expanded&&root.page!=7:HorizontalBox {padding:0px;spacing:8px;vertical-stretch:0;height:root.page==5?60px:42px;
                VerticalBox {padding:0px;spacing:4px;vertical-stretch:0;
                    Text {text:root.tr(root.page==0?"设备连接与管理":root.page==5?"远程桌面":root.page==6?"文件管理与传输":root.page==3?"本地运行日志":"赛台",root.english);font-size:25px;font-weight:600;color:AppColors.ink;vertical-stretch:0;}
                    if root.page==5:Text {text:root.device-name+" · "+root.display;font-size:11px;color:AppColors.muted;vertical-stretch:0;}
                }
                Rectangle {background:transparent;horizontal-stretch:1;}
                if root.page==5:HorizontalBox {padding:0px;spacing:8px;
                    ActionButton {text:root.tr(root.desktop-active&&!root.desktop-ready?"取消连接":root.desktop-ready?"重新连接桌面":"连接桌面",root.english);icon:@image-url("icons/connect.svg");has-icon:true;primary:true;enabled:root.can-interact;clicked=>{if root.desktop-active&&!root.desktop-ready{root.desktop-action("stop");}else{if root.desktop-ready{root.desktop-action("stop");}root.desktop-action("start");}}}
                    ActionButton {text:root.tr("断开桌面",root.english);enabled:root.can-interact&&(root.desktop-active||root.desktop-wanted);clicked=>{root.desktop-action("stop");}}
                }
                if root.page==3:ActionButton {text:root.tr("刷新",root.english);icon:@image-url("icons/restart.svg");has-icon:true;enabled:root.can-browse;clicked=>{root.refresh-logs();}}
                if root.page==6:ConnectionBadge {text:root.tr(root.file-remote-loaded&&root.connected?"SFTP 已连接":"SFTP 未连接",root.english);phase:root.file-remote-loaded&&root.connected?2:0;}
                if root.page==0:HorizontalBox {padding:0px;spacing:8px;
                    ActionButton {text:root.tr("导入旧版资料",root.english);busy:root.migration-busy;enabled:root.can-edit;clicked=>{root.migration-action(false);}}
                    ActionButton {text:root.tr("恢复导入前资料",root.english);busy:root.migration-busy;enabled:root.can-edit;clicked=>{root.migration-action(true);}}
                    ActionButton {text:root.tr("添加设备",root.english);icon:@image-url("icons/plus.svg");has-icon:true;enabled:root.can-edit;clicked=>{root.new-device();}}
                }
            }
            if root.page==0:device-page:=DevicesView {
                observe-motion:root.observe-motion;motion(a,c,p,w,b)=>{root.observed-advanced=a;root.observed-connection=c;root.observed-busy-phase=p;root.observed-busy-wave=w;root.observed-busy=b;}
                english:root.english;editable:root.can-edit;enabled:root.can-interact;busy:root.busy;connected:root.connected;discovering:root.discovering;devices:root.devices;found:root.found;
                selected-id<=>root.selected-id;name<=>root.device-name;host<=>root.host;port<=>root.port;username<=>root.username;auth<=>root.auth;key-path<=>root.key-path;password<=>root.password;remember<=>root.remember;
                advanced<=>root.advanced;display<=>root.display;xauthority<=>root.xauthority;error:root.error;detail:root.connection-detail;stage:root.connection-stage;details-open:root.connection-details-open;discovery-progress:root.discovery-progress;
                tr(s,e)=>{return root.tr(s,e);}select-device(id)=>{root.select-device(id);}select-found(host)=>{root.select-found(host);}discover=>{root.discover();}cancel-discovery=>{root.cancel-discovery();}choose-key=>{root.choose-key();}
                connect=>{root.connect-device();}disconnect=>{root.disconnect-device();}save=>{root.save-device();}remove=>{root.remove-device();}desktop=>{root.page=5;root.desktop-action("start");}
            }
            if root.page==3:LogsView {
                compact:root.compact-content;english:root.english;enabled:root.can-browse;loading:root.logs-loading;
                projects:root.log-projects;days:root.log-days;project-index<=>root.log-project-index;day-index<=>root.log-day-index;status-index<=>root.log-status-index;
                rows:root.log-rows;selectable:root.logs-selectable;all-checked<=>root.logs-all-checked;checked:root.logs-checked;
                back:root.logs-back;next:root.logs-next;summary:root.logs-summary;notice:root.logs-notice;message:root.logs-message;error:root.logs-error;
                selected:root.log-selected;reading:root.log-reading;command:root.log-command;output:root.log-output;warning:root.log-warning;file:root.log-file;left-facts:root.log-left-facts;right-facts:root.log-right-facts;
                tr(s,e)=>{return root.tr(s,e);}filter(p,d,s)=>{root.filter-logs(p,d,s);}refresh=>{root.refresh-logs();}select(id)=>{root.select-log(id);}toggle(id,c)=>{root.toggle-log(id,c);}all(c)=>{root.select-all-logs(c);}page(next)=>{root.log-page(next);}remove=>{root.remove-logs();}open=>{root.open-log-directory(true);}
            }
            if root.page==7:ScrollView {viewport-width:self.visible-width;viewport-height:max(self.visible-height,arena-content.natural-height);vertical-scrollbar-policy:always-off;horizontal-scrollbar-policy:always-off;
            arena-content:=ArenaView {x:0px;y:0px;width:parent.width;height:max(parent.height,self.natural-height);
                english:root.english;enabled:root.can-interact;connected:root.connected;
                cameras:root.arena-cameras;camera-index<=>root.arena-camera-index;rate<=>root.arena-rate;
                camera-active:root.arena-camera-active;camera-loading:root.arena-camera-loading;frame:root.arena-frame;has-frame:root.arena-has-frame;
                camera-status:root.arena-camera-status;metrics-status:root.arena-metrics-status;metric-rows:root.arena-metric-rows;cpu-path:root.arena-cpu-path;memory-path:root.arena-memory-path;
                live:root.arena-pose-live;world-x:root.arena-world-x;world-y:root.arena-world-y;heading:root.arena-heading;has-heading:root.arena-has-heading;pose-status:root.arena-pose-status;coordinates:root.arena-coordinates;
                tr(s,e)=>{return root.tr(s,e);}toggle-camera=>{root.arena-toggle-camera();}refresh-cameras=>{root.arena-refresh-cameras();}plot-resized(ratio)=>{root.arena-plot-changed(ratio);}
            }
            }
            if root.page==6:FilesView {compact:root.compact-content;
                english:root.english;enabled:root.can-interact;connected:root.connected;busy:root.file-mutating;
                geometry-ticket:root.file-geometry-ticket;drives:root.file-drives;drive-index<=>root.file-drive-index;drive(n)=>{root.files-drive(n);}geometry(i,a)=>{if i==0{root.file-local-area=a;}else{root.file-remote-area=a;}}
                drop-side:root.file-drop-side;drop-row:root.file-drop-row;drop-label:root.file-drop-label;
                local-rows:root.file-local-rows;remote-rows:root.file-remote-rows;
                local-path<=>root.file-local-path;remote-path<=>root.file-remote-path;
                local-folder<=>root.file-local-folder;remote-folder<=>root.file-remote-folder;
                local-message:root.file-local-message;remote-message:root.file-remote-message;
                local-loading:root.file-local-loading;remote-loading:root.file-remote-loading;
                local-loaded:root.file-local-loaded;remote-loaded:root.file-remote-loaded;
                local-selected:root.file-local-selected;remote-selected:root.file-remote-selected;
                status:root.file-status;current:root.file-current;numbers:root.file-numbers;error:root.file-error;progress:root.file-progress;rate<=>root.file-rate;
                tr(s,e)=>{return root.tr(s,e);}action(i,a)=>{root.files-action(i,a);}choose(i,n,c,s)=>{root.files-choose(i,n,c,s);}all(i,c)=>{root.files-all(i,c);}open(i,n)=>{root.files-open(i,n);}
            }
            if root.page==5:DesktopView {
                expanded:root.desktop-expanded;toggle-expanded=>{root.desktop-expand(!root.desktop-expanded);}tools-open-changed(open)=>{root.desktop-tools-open=open;}
                toolbar-material:root.material-header;material-active:root.material-active;material-solid:root.material-solid;material-strength:root.bg-material;
                english:root.english;enabled:root.can-interact;connected:root.connected;active:root.desktop-active;wanted:root.desktop-wanted;ready:root.desktop-ready;
                mode:root.desktop-mode;fit<=>root.desktop-fit;read-only<=>root.desktop-readonly;frame:root.desktop-frame;source-width:root.desktop-source-width;source-height:root.desktop-source-height;
                cursor:root.desktop-cursor;cursor-known:root.desktop-cursor-known;cursor-width:root.desktop-cursor-width;cursor-height:root.desktop-cursor-height;cursor-hot-x:root.desktop-cursor-hot-x;cursor-hot-y:root.desktop-cursor-hot-y;
                status:root.desktop-status;error:root.desktop-error;clipboard-available:root.desktop-clipboard-available;focus-revision:root.desktop-focus-revision;
                tr(s,e)=>{return root.tr(s,e);}action(name)=>{root.desktop-action(name);}quality(index)=>{root.set-desktop-mode(index);}readonly-changed=>{root.desktop-readonly-changed();}reboot=>{root.system-action("reboot");}
                pointer(x,y,w,h,b,d)=>{root.desktop-pointer(x,y,w,h,b,d);}motion(x,y,w,h)=>{root.desktop-motion(x,y,w,h);}wheel(x,y,w,h,b,n)=>{root.desktop-wheel(x,y,w,h,b,n);}
                key(value,ctrl,alt,shift)=>{return root.desktop-key(value,ctrl,alt,shift);}text(value)=>{root.desktop-text(value);}focused(value)=>{root.desktop-focused(value);}
            }

                }
            }
            resize-handle:=Rectangle {visible:root.terminal-visible;x:terminal-panel.x - 12px;y:76px;width:12px;height:parent.height - 92px;background:transparent;
                property <length> origin;property <length> original-width;
                accessible-role:slider;accessible-label:root.tr("调整终端侧栏宽度",root.english);accessible-value:round(root.drawer-width/1px);accessible-value-minimum:320;accessible-value-maximum:root.drawer-maximum/1px;
                accessible-action-increment=>{root.drawer-preferred=min(root.drawer-maximum,root.drawer-width+24px);}accessible-action-decrement=>{root.drawer-preferred=max(320px,root.drawer-width - 24px);}
                forward-focus:resize-keys;
                Rectangle {x:4px;y:parent.height/2 - 22px;width:4px;height:44px;border-radius:2px;background:resize-touch.has-hover||resize-keys.has-focus?AppColors.accent.with-alpha(0.35):transparent;}
                resize-keys:=FocusScope {enabled:root.terminal-visible&&root.can-interact&&!root.settings-open;key-pressed(event)=>{if event.text==Key.LeftArrow{root.drawer-preferred=min(root.drawer-maximum,root.drawer-width+24px);return accept;}if event.text==Key.RightArrow{root.drawer-preferred=max(320px,root.drawer-width - 24px);return accept;}if event.text==Key.Home{root.drawer-preferred=320px;return accept;}if event.text==Key.End{root.drawer-preferred=root.drawer-maximum;return accept;}return reject;}}
                resize-touch:=TouchArea {mouse-cursor:ew-resize;changed pressed=>{root.drawer-resizing=self.pressed;}pointer-event(event)=>{if event.kind==PointerEventKind.down&&event.button==PointerEventButton.left{resize-keys.focus();resize-handle.origin=self.absolute-position.x+self.mouse-x;resize-handle.original-width=root.drawer-width;}}moved=>{if self.pressed{root.drawer-preferred=min(root.drawer-maximum,max(320px,resize-handle.original-width + resize-handle.origin - self.absolute-position.x - self.mouse-x));}}double-clicked=>{root.drawer-preferred=-1px;}}
            }
            terminal-panel:=MaterialPanel {visible:root.page!=0&&!root.desktop-expanded&&(root.terminal-open||root.drawer-space>0px);opacity:min(1,root.drawer-space/(root.drawer-width+14px));x:parent.width - root.drawer-space - 2px;y:76px;width:root.drawer-width;height:parent.height - 92px;material:root.material-terminal;active:root.material-active;strength:root.bg-material;solid:root.material-solid;
                VerticalBox {padding:12px;spacing:8px;
                    HorizontalBox {padding:0px;vertical-stretch:0;
                        Text {text:root.english?"Terminal":"终端";font-size:18px;font-weight:600;color:Palette.foreground;vertical-alignment:center;}
                        ActionButton {text:root.english?"Hide":"收起";width:80px;horizontal-stretch:0;clicked=>{root.terminal-open=false;}}
                    }
                    TerminalView {
                tabs:root.terminal-tabs;active:root.terminal-active;cells:root.terminal-cells;
                cursor-column:root.terminal-cursor-column;cursor-row:root.terminal-cursor-row;cursor-visible:root.terminal-cursor-visible;
                phase:root.terminal-phase;status:root.terminal-status;info:root.terminal-information;error:root.terminal-error;log-status:root.terminal-log-status;
                connected:root.connected;busy:root.terminal-busy;sending:root.terminal-sending;enabled:root.can-interact;english:root.english;
                draft<=>root.terminal-draft;project<=>root.terminal-project;
                completions:root.terminal-completions;completion-index:root.terminal-completion-index;
                completion-loading:root.terminal-completion-loading;completion-truncated:root.terminal-completion-truncated;
                command-caret:root.terminal-command-caret;command-revision:root.terminal-command-revision;
                tr(s,e)=>{return root.tr(s,e);}
                close-tab(key)=>{root.terminal-close-tab(key);}select-tab(key)=>{root.terminal-select(key);}action(name)=>{root.terminal-action(name);}connect-device=>{root.page=0;}
                copy=>{root.terminal-copy();}paste=>{root.terminal-paste();}scroll(n)=>{root.terminal-scroll(n);}
                select(x,y,x2,y2)=>{root.terminal-selection(x,y,x2,y2);}resize-grid(x,y)=>{root.terminal-resize(x,y);}
                committed(value)=>{root.terminal-committed(value);}key(value,ctrl,alt,shift)=>{return root.terminal-key(value,ctrl,alt,shift);}
                draft-edited(value,cursor)=>{root.terminal-draft-edited(value,cursor);}project-edited(value)=>{root.terminal-project-edited(value);}
                editor-key(value,ctrl,shift,alt,expanded)=>{return root.terminal-editor-key(value,ctrl,shift,alt,expanded);}
                editor-focused(value)=>{root.terminal-editor-focused(value);}choose-completion(index)=>{root.terminal-choose-completion(index);}
                send-command=>{root.terminal-send-command();}
            }
                }
            }
        }
        Rectangle {visible:root.settings-open||root.settings-progress>0;opacity:root.settings-progress;background:transparent;
            // Retain the bounded backdrop until the sheet closes; preference
            // edits must not turn a blurred modal backdrop into the sharp page.
            if root.settings-backdrop-active&&!root.material-solid:Image {x:0px;y:0px;width:parent.width;height:parent.height;opacity:root.settings-backdrop-phase;source:root.settings-backdrop-frame;image-fit:fill;}
            Rectangle {background:#23345030;}
            TouchArea {enabled:root.settings-open&&root.can-interact;clicked=>{root.settings-open=false;}}
            settings-panel:=MaterialPanel {x:(parent.width - self.width)/2;y:(parent.height - self.height)/2+(1-root.settings-progress)*18px;width:min(840px,parent.width - 64px);height:min(640px,parent.height - 64px);solid:true;
                transform-origin:{x:self.width/2,y:self.height/2};transform-scale:0.96+0.04*root.settings-progress;
                TouchArea {}
                settings-focus:=FocusScope {enabled:root.dialog-kind==0&&root.settings-open;
                    FocusScope {width:0px;height:0px;changed has-focus=>{if self.has-focus{settings-close.focus();}}}
                    key-pressed(event)=>{if event.text==Key.Escape{root.settings-open=false;return accept;}return reject;}
                    HorizontalBox {padding:0px;spacing:0px;
                        Rectangle {width:174px;horizontal-stretch:0;background:transparent;
                            VerticalBox {padding-left:16px;padding-right:16px;padding-top:24px;padding-bottom:24px;spacing:16px;
                                Text {text:root.tr("设置",root.english);font-size:20px;font-weight:600;color:AppColors.ink;height:44px;vertical-stretch:0;}
                                Rectangle {height:142px;vertical-stretch:0;background:transparent;accessible-role:tab-list;
                                    settings-pill:=Rectangle {transform-origin:{x:self.width/2,y:self.height/2};transform-scale:root.settings-section==0?settings-general.press-scale:root.settings-section==1?settings-storage.press-scale:settings-about.press-scale;y:root.settings-section*50px;width:parent.width;height:42px;border-radius:12px;background:AppColors.field;animate y {duration:AppColors.motion?320ms:0ms;easing:cubic-bezier(0.16,1,0.3,1);}}
                                    settings-general:=SettingsTab {y:0px;width:parent.width;text:root.tr("通用",root.english);icon:@image-url("icons/settings.svg");selected:root.settings-section==0;enabled:root.can-interact;clicked=>{root.settings-section=0;}}
                                    settings-storage:=SettingsTab {y:50px;width:parent.width;text:root.tr("存储",root.english);icon:@image-url("icons/files.svg");selected:root.settings-section==1;enabled:root.can-interact;clicked=>{root.settings-section=1;}}
                                    settings-about:=SettingsTab {y:100px;width:parent.width;text:root.tr("关于",root.english);icon:@image-url("icons/about.svg");selected:root.settings-section==2;enabled:root.can-interact;clicked=>{root.settings-section=2;}}
                                }
                                Rectangle {background:transparent;vertical-stretch:1;}
                            }
                        }
                        VerticalBox {padding:24px;spacing:20px;
                            HorizontalBox {padding:0px;vertical-stretch:0;
                                Text {text:root.tr(root.settings-section==0?"通用":root.settings-section==1?"存储":"关于",root.english);font-size:18px;font-weight:600;color:Palette.foreground;}
                                Rectangle {background:transparent;horizontal-stretch:1;}
                            }
                            if root.settings-error!="":Text {text:root.tr(root.settings-error,root.english);color:Palette.foreground;font-weight:600;wrap:word-wrap;vertical-stretch:0;}
                            if root.logs-error!=""&&root.settings-section==1:Text {text:root.tr(root.logs-error,root.english);color:Palette.foreground;wrap:word-wrap;vertical-stretch:0;}
                            ScrollView {viewport-width:self.visible-width;viewport-height:settings-content.preferred-height;vertical-scrollbar-policy:always-off;horizontal-scrollbar-policy:always-off;
                                settings-content:=VerticalBox {y:(1-root.settings-content-phase)*5px;min-width:0px;padding:0px;spacing:18px;
                                    if root.settings-section==0:VerticalBox {padding:0px;spacing:18px;
                    HorizontalBox {padding:0px;height:42px;vertical-stretch:0;Text {text:root.tr("语言",root.english);color:AppColors.ink;font-size:13px;vertical-alignment:center;}ComboBox {width:150px;horizontal-stretch:0;model:["简体中文","English"];current-index:root.english?1:0;enabled:root.can-interact&&root.settings-open;selected(index)=>{root.set-language(index);}}}
                    HorizontalBox {padding:0px;height:42px;vertical-stretch:0;Text {text:root.tr("主题色彩",root.english);color:AppColors.ink;font-size:13px;vertical-alignment:center;}ComboBox {width:150px;horizontal-stretch:0;model:[root.tr("浅蓝",root.english),root.tr("绿色",root.english),root.tr("石墨",root.english)];current-index:root.theme;enabled:root.can-interact&&root.settings-open;selected(index)=>{root.set-theme(index);}}}
                    HorizontalBox {padding:0px;height:42px;vertical-stretch:0;Text {text:root.tr("外观",root.english);color:AppColors.ink;font-size:13px;vertical-alignment:center;}SegmentedChoice {width:260px;model:[root.tr("浅色",root.english),root.tr("深色",root.english),root.tr("跟随系统",root.english)];current-index:root.appearance;enabled:root.can-interact&&root.settings-open;selected(index)=>{root.set-appearance(index);}}}
                    GlassSettings {english:root.english;enabled:root.glass-loaded&&root.can-interact&&!root.bg-busy;quality<=>root.glass-quality;material<=>root.bg-material;error:root.glass-error;notice:root.material-status;tr(s,e)=>{return root.tr(s,e);}update(finished)=>{root.glass-controls(finished);}}
                    background-settings:=BackgroundSettings {focus-ticket:root.color-focus-ticket;compact:true;video:root.bg-video;english:root.english;enabled:root.can-interact&&!root.busy;busy:root.bg-busy;cancellable:root.bg-cancellable;
                        frame:root.bg-frame;has-frame:root.bg-has-frame;color:root.bg-color;source-width:root.bg-width;source-height:root.bg-height;
                        crop-x:root.bg-saved-crop-x;crop-y:root.bg-saved-crop-y;crop-width:root.bg-saved-crop-width;crop-height:root.bg-saved-crop-height;
                        hex<=>root.bg-hex;volume<=>root.bg-volume;status:root.bg-status;error:root.bg-error;
                        tr(s,e)=>{return root.tr(s,e);}picker-position(x,y)=>{root.color-anchor-x=x*1px;root.color-anchor-y=y*1px;}action(s)=>{root.bg-action(s);}controls=>{root.bg-controls();}
                    }

                                    }
                                    if root.settings-section==1:VerticalBox {padding:0px;spacing:12px;
                        Text {text:root.tr("板端运行环境",root.english);font-size:14px;font-weight:600;color:AppColors.ink;}
                        environment-computer:=EnvironmentOption {text:root.tr("电脑下载后传输",root.english);detail:root.tr("电脑联网下载并缓存，通过 SSH 传给设备",root.english);selected:root.environment-source==0;enabled:root.can-interact;clicked=>{root.set-environment(0);}move=>{root.set-environment(1);environment-board.focus();}}
                        environment-board:=EnvironmentOption {text:root.tr("板端自行下载",root.english);detail:root.tr("设备通过自己的网络访问软件源",root.english);selected:root.environment-source==1;enabled:root.can-interact;clicked=>{root.set-environment(1);}move=>{root.set-environment(0);environment-computer.focus();}}
                        Rectangle {height:1px;vertical-stretch:0;background:AppColors.edge;}
                        Text {text:root.tr("运行日志",root.english);font-size:14px;font-weight:600;color:AppColors.ink;}
                        Rectangle {height:106px;vertical-stretch:0;background:AppColors.inset;border-radius:12px;border-width:1px;border-color:AppColors.edge;
                            VerticalBox {padding:12px;spacing:8px;
                                Text {text:root.tr("保存位置",root.english);font-size:11px;color:AppColors.muted;}
                                Text {text:root.log-directory;color:AppColors.ink;font-size:11px;overflow:elide;}
                                HorizontalBox {padding:0px;spacing:8px;vertical-stretch:0;
                                    settings-open-directory:=ActionButton {text:root.tr("打开目录",root.english);enabled:!root.logs-mutating&&root.can-interact;clicked=>{root.open-log-directory(false);}}
                                    ActionButton {text:root.tr("更换位置",root.english);enabled:!root.logs-mutating&&root.can-interact;clicked=>{root.choose-log-directory();}}
                                    Rectangle {background:transparent;horizontal-stretch:1;}
                                }
                            }
                        }
                        Text {text:root.tr("存储上限（MiB）",root.english);font-size:11px;color:AppColors.muted;}
                        HorizontalBox {padding:0px;spacing:8px;vertical-stretch:0;
                            LineEdit {text<=>root.log-limit;enabled:root.can-interact;label:root.tr("日志上限（MiB）",root.english);}
                            ActionButton {text:root.tr("保存上限",root.english);enabled:root.can-interact;clicked=>{root.save-log-limit(root.log-limit);}}
                            Text {text:root.log-usage-known?root.tr("已用",root.english)+" "+root.log-usage:root.logs-loading?root.tr("正在读取…",root.english):"";font-size:11px;color:AppColors.muted;vertical-alignment:center;horizontal-stretch:0;}
                        }
                        Text {text:root.tr("留空表示不限；自动清理最早的已结束日志，保留正在记录的日志。",root.english);font-size:11px;color:AppColors.muted;wrap:word-wrap;}
                    }
                                    if root.settings-section==2:VerticalBox {padding:0px;spacing:16px;
                        VerticalBox {padding-top:12px;padding-bottom:16px;padding-left:0px;padding-right:0px;spacing:12px;vertical-stretch:0;
                            HorizontalBox {padding:0px;Rectangle {background:transparent;horizontal-stretch:1;}Image {source:@image-url("../assets/portcanvas.png");width:76px;height:76px;horizontal-stretch:0;vertical-stretch:0;}Rectangle {background:transparent;horizontal-stretch:1;}}
                            Text {text:"PortCanvas Beta";font-size:24px;font-weight:600;color:AppColors.ink;horizontal-alignment:center;}
                            HorizontalBox {padding:0px;Rectangle {background:transparent;horizontal-stretch:1;}Rectangle {preferred-width:version-label.preferred-width+20px;height:28px;border-radius:14px;background:AppColors.soft;horizontal-stretch:0;version-label:=Text {x:10px;width:parent.width - 20px;height:parent.height;text:"v"+root.app-version;font-size:11px;color:AppColors.muted;vertical-alignment:center;}}Rectangle {background:transparent;horizontal-stretch:1;}}
                        }
                        Rectangle {height:1px;vertical-stretch:0;background:AppColors.edge;}
                        Text {text:root.tr("GitHub 仓库",root.english);font-size:13px;font-weight:500;color:AppColors.ink;}
                        Text {text:"JvKan9Zz/PortCanvas";font-size:12px;color:AppColors.muted;}
                        HorizontalBox {padding:0px;spacing:8px;vertical-stretch:0;
                            ActionButton {text:root.tr("打开仓库",root.english);enabled:root.can-interact;clicked=>{root.system-action("repository");}}
                            ActionButton {text:root.tr(root.update-busy?"取消更新":root.update-phase=="available"?"下载更新":root.update-phase=="ready"?"安装并重启":"检查更新",root.english);primary:root.update-phase=="available"||root.update-phase=="ready";busy:root.update-busy;enabled:root.can-interact&&(root.update-phase!="ready"||!root.file-mutating);clicked=>{root.update-action(root.update-busy?"cancel":root.update-phase=="available"?"download":root.update-phase=="ready"?"install":"check");}}
                            ActionButton {text:root.tr("打开安装目录",root.english);enabled:root.can-interact;clicked=>{root.system-action("directory");}}
                            Rectangle {background:transparent;horizontal-stretch:1;}
                        }
                        Text {text:root.update-version!=""?"v"+root.update-version:root.tr(root.update-phase=="current"?"当前已是最新版本":root.update-phase=="checking"?"正在检查更新…":root.update-phase=="downloading"?"正在下载更新…":root.update-phase=="ready"?"更新已校验，可安装":root.update-phase=="canceled"?"更新已取消。":"",root.english);color:AppColors.muted;font-size:12px;wrap:word-wrap;}
                        if root.update-message!="":Text {text:root.update-message;color:AppColors.danger;wrap:word-wrap;}
                        if root.update-notes!="":Text {text:root.update-notes;color:AppColors.ink;wrap:word-wrap;}
                        if root.update-phase=="downloading":Rectangle {height:4px;vertical-stretch:0;background:AppColors.edge;Rectangle {x:0px;y:0px;width:parent.width*root.update-progress;background:AppColors.accent;animate width {duration:AppColors.motion?160ms:0ms;}}}
                        third-party-entry:=DesktopTool {text:root.tr("第三方组件",root.english);icon:@image-url("icons/chevron-right.svg");icon-angle:root.third-party-open?90deg:0deg;enabled:root.can-interact;clicked=>{root.third-party-open=!root.third-party-open;}}
                        if root.third-party-open:AboutSlint {preferred-height:120px;vertical-stretch:0;}
                    }
                                }
                            }
                        }
                    }
                    settings-close:=DesktopTool {x:parent.width - 68px;y:16px;width:44px;height:44px;show-text:false;label:root.tr("关闭设置",root.english);icon:@image-url("icons/close.svg");enabled:root.can-interact;clicked=>{root.settings-open=false;}}
                    FocusScope {width:0px;height:0px;changed has-focus=>{if self.has-focus{settings-general.focus();}}}
                }
            }
        }
        Rectangle {visible:root.quick-open||root.quick-phase>0;opacity:root.quick-phase;background:#23345030;TouchArea {}
            Rectangle {x:(parent.width - self.width)/2;y:(parent.height - self.height)/2+(1-root.quick-phase)*10px;width:min(440px,parent.width - 64px);height:quick-view.preferred-height;background:AppColors.surface;border-radius:24px;border-width:1px;border-color:AppColors.edge;drop-shadow-offset-y:12px;drop-shadow-blur:32px;drop-shadow-color:#223a6624;
                FocusScope {enabled:root.quick-open&&root.dialog-kind==0;key-pressed(event)=>{if event.text==Key.Escape{root.quick-cancel();return accept;}return reject;}
                    quick-view:=QuickConnect {english:root.english;busy:root.busy;enabled:root.dialog-kind==0;key-auth:root.auth==1;target:root.host+":"+root.port;error:root.error;name<=>root.device-name;username<=>root.username;password<=>root.password;remember<=>root.remember;tr(s,e)=>{return root.tr(s,e);}submit=>{root.quick-submit();}cancel=>{root.quick-cancel();}}
                }
            }
        }
        Rectangle {
            visible:root.painted-dialog!=0;opacity:root.dialog-phase;
            background:root.painted-dialog==14?transparent:#00000070;
            TouchArea {clicked=>{if root.painted-dialog==14{root.bg-picker-response(false);}}}
            modal-panel:=Rectangle {
                width:root.painted-dialog==14?286px:root.painted-dialog==8?min(800px,root.width - 64px):min(root.width - 64px,640px);height:root.painted-dialog==8?min(560px,root.height - 64px):root.painted-dialog==14?picker-view.preferred-height:panel.preferred-height;
                x:root.painted-dialog==14?min(root.width - self.width - 16px,max(16px,root.color-anchor-x)):(root.width - self.width)/2;y:root.painted-dialog==14?min(root.height - self.height - 16px,max(16px,root.color-anchor-y>self.height+16px?root.color-anchor-y - self.height - 8px:root.color-anchor-y+38px)):(root.height - self.height)/2+(1-root.dialog-phase)*6px;
                background:AppColors.surface;border-radius:root.painted-dialog==14?14px:24px;drop-shadow-offset-y:12px;drop-shadow-blur:32px;drop-shadow-color:#223a6624;border-width:1px;border-color:AppColors.edge;
                modal-focus:=FocusScope {enabled:root.dialog-kind!=0;
                    FocusScope {width:0px;height:0px;enabled:root.painted-dialog!=8&&root.painted-dialog!=14;changed has-focus=>{if self.has-focus{confirm-response.focus();}}}
                    key-pressed(event)=>{if root.painted-dialog==14{if event.text==Key.Escape{root.bg-picker-response(false);return accept;}return reject;}if root.painted-dialog==8{if event.text==Key.Escape{root.bg-crop-response(false);return accept;}return reject;}if event.text==Key.Escape{root.respond(false);return accept;}return reject;}
                    picker-view:=ColorPicker {visible:root.painted-dialog==14;english:root.english;selected-color:root.bg-picker-color;hue-color:root.bg-picker-hue-color;hue:root.bg-picker-hue;saturation:root.bg-picker-saturation;brightness:root.bg-picker-brightness;
                        red:root.bg-picker-red;green:root.bg-picker-green;blue:root.bg-picker-blue;hex<=>root.bg-picker-hex;error:root.bg-picker-error;
                        tr(s,e)=>{return root.tr(s,e);}change(h,s,v)=>{root.bg-picker-change(h,s,v);}rgb(r,g,b)=>{root.bg-picker-rgb(r,g,b);}hex-edited(value)=>{root.bg-picker-hex-edited(value);}response(value)=>{root.bg-picker-response(value);}
                    }
                    crop-view:=BackgroundCropView {visible:root.painted-dialog==8;english:root.english;busy:root.bg-busy;frame:root.bg-preview;
                        source-width:root.bg-preview-width;source-height:root.bg-preview-height;crop-x:root.bg-crop-x;crop-y:root.bg-crop-y;crop-width:root.bg-crop-width;crop-height:root.bg-crop-height;error:root.bg-error;
                        tr(s,e)=>{return root.tr(s,e);}response(v)=>{root.bg-crop-response(v);}reset=>{root.bg-crop-reset();}gesture(k,x,y)=>{root.bg-crop-gesture(k,x,y);}arrow(x,y,r)=>{root.bg-crop-arrow(x,y,r);}
                    }
                    panel:=VerticalBox {visible:root.painted-dialog!=8&&root.painted-dialog!=14;padding:24px;spacing:18px;
                        Text {text:root.tr(root.dialog-title,root.english);font-size:20px;font-weight:600;color:Palette.foreground;}
                        Text {text:root.migration-executing?root.tr("正在迁移资料…",root.english):root.system-executing?root.tr("正在提交重启请求…",root.english):root.dialog-message;wrap:word-wrap;color:Palette.foreground;}
                        folder-name:=LineEdit {visible:root.painted-dialog==15;label:root.tr("文件夹名称",root.english);text<=>root.file-folder-name;enabled:root.dialog-kind==15&&!root.file-mutating;accepted=>{root.respond(true);}}
                        if root.painted-dialog==15&&root.file-error!="":Text {text:root.file-error;color:AppColors.danger;wrap:word-wrap;}
                        HorizontalBox {padding:0px;spacing:8px;
                            Rectangle {background:transparent;horizontal-stretch:1;}
                            cancel-response:=ActionButton {enabled:!root.system-executing&&!root.migration-executing&&!(root.painted-dialog==15&&root.file-mutating);text:root.tr(root.painted-dialog==12?"保持当前":"取消",root.english);primary:false;clicked=>{root.respond(false);}}
                            confirm-response:=ActionButton {busy:root.painted-dialog==15&&root.file-mutating;enabled:!root.system-executing&&!root.migration-executing&&(root.painted-dialog!=15||(!root.file-mutating&&root.file-folder-name!=""));text:root.tr(root.painted-dialog==15?"创建":root.painted-dialog==13?"安装并重启":root.painted-dialog==12?(root.material-suggestion-target==1?"切换到均衡":"切换到轻量"):root.painted-dialog==2?"信任并连接":root.painted-dialog==3?"清理所选":(root.painted-dialog==4||root.painted-dialog==9)?"确认重启":root.painted-dialog==5?"确认关闭":root.painted-dialog==6?"确认覆盖":root.painted-dialog==7?"确认删除":root.painted-dialog==10?"确认导入":root.painted-dialog==11?"确认恢复":"确认移除",root.english);primary:root.painted-dialog==15||root.painted-dialog==2||root.painted-dialog==8||root.painted-dialog==10||root.painted-dialog==13;danger:root.painted-dialog==3||root.painted-dialog==4||root.painted-dialog==5||root.painted-dialog==6||root.painted-dialog==7||root.painted-dialog==9||root.painted-dialog==11;clicked=>{root.respond(true);}}
                            FocusScope {width:0px;height:0px;enabled:root.painted-dialog!=8&&root.painted-dialog!=14;changed has-focus=>{if self.has-focus{cancel-response.focus();}}}
                        }
                    }
                }
            }
        }
        Rectangle {visible:root.toast-open||root.toast-phase>0;x:root.width - self.width - 30px;y:root.height - self.height - 28px+(1-root.toast-phase)*12px;width:min(440px,root.width - 160px);height:toast-text.preferred-height+28px;opacity:root.toast-phase;background:#22324bf0;border-radius:15px;border-width:1px;border-color:#ffffff33;drop-shadow-offset-y:8px;drop-shadow-blur:26px;drop-shadow-color:#1b2d4b30;
            toast-text:=Text {x:18px;y:14px;width:parent.width - 36px;text:root.toast-message;color:#f2f6fc;font-size:13px;wrap:word-wrap;}
            TouchArea {}
        }

    }
}
