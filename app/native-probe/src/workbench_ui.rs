slint::slint! {
    import {Button,LineEdit,ComboBox,CheckBox,ScrollView,ListView,TextEdit,VerticalBox,HorizontalBox,Palette,AboutSlint} from "std-widgets.slint";
    import {TerminalView,TerminalCell,TerminalTab,CompletionRow} from "terminal_view.slint";
    import {DesktopView} from "desktop_view.slint";
    import {GlassSettings} from "glass_view.slint";
    import {BackgroundSettings,BackgroundCropView} from "background_view.slint";
    import {ArenaView} from "arena_view.slint";
    import {FilesView,FileRow,FileArea} from "files_view.slint";
    import {ActionButton,AppColors} from "action_button.slint";
    export struct LogRow {id:string,command:string,context:string,status:string,removable:bool,checked:bool,selected:bool}
    component LogSelectionBox inherits CheckBox {
        in property <bool> selected:false;
        checked:root.selected;
        changed selected=>{root.checked=root.selected;}
    }
    export struct DeviceRow {id:string,name:string,address:string}
    export component Workbench inherits Window {
        title:"PortCanvas · Demo V0.6.0 (dev)";
        preferred-width:1200px;preferred-height:820px;min-width:1000px;min-height:700px;
        default-font-family:"Microsoft YaHei UI";default-font-size:14px;
        background:Palette.background;
        no-frame:root.window-is-fullscreen;
        init=>{Palette.color-scheme=ColorScheme.light;}
        pure callback tr(string,bool)->string;
        public function focus-main(){main-focus.focus();}
        in-out property <bool> english:false;
        in-out property <int> page:0;
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
        in-out property <string> display:":0";
        in-out property <string> xauthority;
        in property <bool> system-busy:false;in property <bool> system-executing:false;in property <string> system-error;callback system-action(string);callback system-confirm(string,string);callback system-finished(string,string);
        in property <bool> busy:false;
        in property <bool> connected:false;
        in property <bool> discovering:false;
        in property <string> discovery-progress;
        in property <string> connection-status:"尚未连接设备";
        in property <string> connection-detail;
        in property <string> error;
        in property <string> data-directory;
        in-out property <int> appearance:0;
        in-out property <int> theme:0;
        in property <bool> window-is-fullscreen:false;
        in property <bool> native-hidden:false;
        in property <[FileRow]> file-local-rows;in property <[FileRow]> file-remote-rows;
        in property <[string]> file-drives;in-out property <int> file-drive-index:-1;callback files-drive(int);
        in-out property <FileArea> file-local-area;in-out property <FileArea> file-remote-area;
        in-out property <int> file-geometry-ticket;
        in property <int> file-drop-side:-1;in property <int> file-drop-row:-1;in property <string> file-drop-label;
        in-out property <string> file-local-path;in-out property <string> file-remote-path;
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
        in property <string> arena-camera-status;in property <string> arena-metrics-status;in property <string> arena-metrics;
        in property <string> arena-cpu-path;in property <string> arena-memory-path;
        in property <bool> arena-pose-live;in property <float> arena-world-x;in property <float> arena-world-y;in property <float> arena-heading;in property <bool> arena-has-heading;
        in property <string> arena-pose-status;in property <string> arena-coordinates;
        callback arena-sync();callback arena-toggle-camera();callback arena-refresh-cameras();callback arena-environment(string);
        callback arena-metrics-result(string,string);callback arena-cameras-result(string,string,string);callback arena-camera-finished(string,string);
        callback arena-pose-started(string,string);callback arena-pose-packet(string);
        in property <image> bg-frame;in property <bool> bg-has-frame;in property <bool> bg-custom;in property <color> bg-color:#eef3fb;
        in property <int> bg-width;in property <int> bg-height;in property <bool> bg-busy;in property <bool> bg-cancellable;
        in-out property <string> bg-hex:"#eef3fb";in-out property <float> bg-volume:0;in-out property <float> bg-material:100;
        in-out property <int> glass-quality:2;in property <bool> glass-loaded:false;in property <string> glass-error;callback glass-controls(bool);
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
        changed connected=>{root.desktop-view-changed();}
        changed english=>{root.arena-sync();root.files-sync();root.desktop-view-changed();}
        in property <[TerminalTab]> terminal-tabs;
        in property <[TerminalCell]> terminal-cells;
        in property <string> terminal-active;
        in property <string> terminal-phase;
        in property <string> terminal-status;
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
        callback terminal-select(string);callback terminal-action(string);callback terminal-copy();callback terminal-paste();
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
        in property <string> log-warning;
        in property <string> log-output;
        property <bool> can-browse:!root.logs-loading&&!root.logs-mutating&&root.can-interact;
        callback refresh-logs();callback poll-logs();callback filter-logs(int,int,int);callback log-page(bool);
        callback select-log(string);callback toggle-log(string,bool);callback select-all-logs(bool);callback remove-logs();
        callback choose-log-directory();callback open-log-directory(bool);callback fullscreen();callback set-theme(int);
        changed theme=>{AppColors.accent=root.theme==1?#2b8169:root.theme==2?#53657f:#356ce5;}
        changed page=>{root.bg-sync();root.arena-sync();root.files-sync();root.desktop-view-changed();root.terminal-cancel-completion();if root.page==3{root.refresh-logs();}if root.page==4{root.terminal-reload();}}
        changed native-hidden=>{root.bg-sync();root.arena-sync();root.files-sync();root.desktop-view-changed();if root.native-hidden{root.terminal-cancel-completion();}if !root.native-hidden&&root.page==3{root.refresh-logs();}if !root.native-hidden&&root.page==4{root.terminal-reload();}}
        Timer {interval:1200ms;running:root.page==3&&root.log-live&&!root.native-hidden&&root.can-interact;triggered=>{root.poll-logs();}}

        in-out property <int> environment-source:0;
        in-out property <int> desktop-mode:0;
        changed desktop-mode=>{root.desktop-quality-changed();}
        in-out property <string> log-limit:"1024";
        in-out property <int> dialog-kind:0;
        property <bool> can-interact:root.dialog-kind==0&&!root.system-busy&&!root.migration-busy;
        in property <bool> migration-busy:false;in property <bool> migration-executing:false;
        in property <string> migration-status;in property <string> migration-error;
        callback migration-action(bool);callback migration-confirm(string,string,string,bool);callback migration-finished(string,string,string);
        in property <string> dialog-title;
        in property <string> dialog-message;
        property <bool> accept-focused:false;
        property <bool> can-edit:!root.busy && !root.connected && !root.logs-mutating && root.can-interact;
        callback select-device(string);callback select-found(string);callback new-device();
        callback save-device();callback connect-device();callback disconnect-device();callback remove-device();
        callback choose-key();callback discover();callback cancel-discovery();callback respond(bool);
        callback set-language(int);callback set-appearance(int);callback set-environment(int);callback set-desktop-mode(int);callback save-log-limit(string);
        changed appearance=>{Palette.color-scheme=root.appearance==0?ColorScheme.light:root.appearance==1?ColorScheme.dark:ColorScheme.unknown;}
        changed dialog-kind=>{root.bg-sync();if root.dialog-kind==8{crop-view.focus-selection();}else if root.dialog-kind!=0 {root.accept-focused=false;modal-focus.focus();}else{main-focus.focus();}}
        Rectangle {background:root.bg-custom?root.bg-color:Palette.background;
            if root.bg-has-frame:Image {source:root.bg-frame;width:parent.width;height:parent.height;image-fit:cover;source-clip-x:round(root.bg-saved-crop-x*root.bg-width);source-clip-y:round(root.bg-saved-crop-y*root.bg-height);source-clip-width:max(1,round(root.bg-saved-crop-width*root.bg-width));source-clip-height:max(1,round(root.bg-saved-crop-height*root.bg-height));}
        }
        Rectangle {background:Palette.background;opacity:root.bg-custom?0.48+0.4*root.bg-material/100:1;}
        main-focus:=FocusScope {enabled:!root.system-busy&&!root.migration-busy; VerticalBox {
            padding:20px;spacing:16px;
            HorizontalBox {
                padding:0px;height:40px;vertical-stretch:0;
                Text {text:"PortCanvas";color:Palette.foreground;font-size:25px;font-weight:650;}
                Button {text:root.tr(root.window-is-fullscreen?"退出全屏":"全屏",root.english);horizontal-stretch:0;preferred-width:110px;enabled:root.can-interact;clicked=>{root.fullscreen();}}
                Text {text:root.tr(root.connection-status,root.english);color:Palette.foreground;horizontal-alignment:right;vertical-alignment:center;}
                Button {text:root.tr(root.busy?"取消连接":"断开连接",root.english);visible:root.busy||root.connected;enabled:root.can-interact;clicked=>{root.disconnect-device();}}
            }
            HorizontalBox {
                padding:0px;height:38px;vertical-stretch:0;
                ActionButton {text:root.tr("设备连接与管理",root.english);primary:root.page==0;enabled:root.can-interact;clicked=>{root.page=0;}}
                ActionButton {text:root.tr("远程桌面",root.english);primary:root.page==5;enabled:root.can-interact;clicked=>{root.page=5;}}
                ActionButton {text:root.tr("文件",root.english);primary:root.page==6;enabled:root.can-interact;clicked=>{root.page=6;}}
                ActionButton {text:root.tr("赛台",root.english);primary:root.page==7;enabled:root.can-interact;clicked=>{root.page=7;}}
                ActionButton {text:root.tr("终端",root.english);primary:root.page==4;enabled:root.can-interact;clicked=>{root.page=4;}}
                ActionButton {text:root.tr("日志",root.english);primary:root.page==3;enabled:root.can-interact;clicked=>{root.page=3;}}
                ActionButton {text:root.tr("设置",root.english);primary:root.page==1;enabled:root.can-interact;clicked=>{root.page=1;}}
                ActionButton {text:root.tr("关于",root.english);primary:root.page==2;enabled:root.can-interact;clicked=>{root.page=2;}}
            }
            if root.system-error!="":Text {text:root.tr(root.system-error,root.english);color:#ce3535;wrap:word-wrap;vertical-stretch:0;}
            if root.page!=1&&root.bg-error!="":Text {text:root.tr(root.bg-error,root.english);color:#ce3535;wrap:word-wrap;vertical-stretch:0;}
            if root.migration-status!="":Text {text:root.tr(root.migration-status,root.english);color:Palette.foreground;wrap:word-wrap;vertical-stretch:0;}
            if root.migration-error!="":Text {text:root.tr(root.migration-error,root.english);color:#ce3535;wrap:word-wrap;vertical-stretch:0;}
            if root.page==0: HorizontalBox {
                padding:0px;spacing:24px;vertical-stretch:1;
                VerticalBox {
                    padding:0px;spacing:10px;width:270px;horizontal-stretch:0;
                    HorizontalBox {padding:0px;height:34px;vertical-stretch:0;
                        Text {text:root.tr("已保存的设备",root.english);color:Palette.foreground;vertical-alignment:center;}
                        Button {text:root.tr("添加设备",root.english);enabled:root.can-edit;clicked=>{root.new-device();}}
                    }
                    saved-scroll:=ScrollView {
                        viewport-height:saved-list.preferred-height;
                        saved-list:=VerticalBox {padding:0px;spacing:6px;
                            for device in root.devices: ActionButton {text:device.name+"\n"+device.address;primary:root.selected-id==device.id;enabled:root.can-edit;preferred-height:58px;vertical-stretch:0;clicked=>{root.select-device(device.id);}}
                            if root.devices.length==0:Text {text:root.tr("暂无设备",root.english);color:Palette.foreground;horizontal-alignment:center;}
                        }
                    }
                    HorizontalBox {padding:0px;height:34px;vertical-stretch:0;
                        Text {text:root.tr("局域网设备",root.english);color:Palette.foreground;vertical-alignment:center;}
                        Button {text:root.tr(root.discovering?"停止搜索":"搜索",root.english);enabled:!root.busy&&root.can-interact;clicked=>{if root.discovering{root.cancel-discovery();}else{root.discover();}}}
                    }
                    Text {text:root.discovery-progress;color:Palette.foreground;wrap:word-wrap;vertical-stretch:0;}
                    found-scroll:=ScrollView {
                        viewport-height:found-list.preferred-height;
                        found-list:=VerticalBox {padding:0px;spacing:6px;
                            for device in root.found:Button {text:device.address;enabled:root.can-edit;preferred-height:40px;vertical-stretch:0;clicked=>{root.select-found(device.id);}}
                        }
                    }
                }
                form-scroll:=ScrollView {
                    viewport-height:form.preferred-height;
                    form:=VerticalBox {padding:0px;spacing:12px;
                        HorizontalBox {padding:0px;height:36px;vertical-stretch:0;
                            Button {text:root.tr("导入旧版资料",root.english);enabled:root.can-edit;clicked=>{root.migration-action(false);}}
                            Button {text:root.tr("恢复导入前资料",root.english);enabled:root.can-edit;clicked=>{root.migration-action(true);}}
                        }
                        Text {text:root.device-name==""?root.tr("添加一台设备",root.english):root.device-name;color:Palette.foreground;font-size:22px;font-weight:600;}
                        Text {text:root.tr("设备名称",root.english);color:Palette.foreground;}
                        LineEdit {text<=>root.device-name;enabled:root.can-edit;}
                        HorizontalBox {padding:0px;spacing:16px;
                            VerticalBox {padding:0px;spacing:8px;horizontal-stretch:3;
                                Text {text:root.tr("设备地址",root.english);color:Palette.foreground;}
                                LineEdit {text<=>root.host;enabled:root.can-edit;}
                            }
                            VerticalBox {padding:0px;spacing:8px;horizontal-stretch:1;
                                Text {text:root.tr("SSH 端口",root.english);color:Palette.foreground;}
                                LineEdit {text<=>root.port;enabled:root.can-edit;}
                            }
                        }
                        HorizontalBox {padding:0px;spacing:16px;
                            VerticalBox {padding:0px;spacing:8px;
                                Text {text:root.tr("用户名",root.english);color:Palette.foreground;}
                                LineEdit {text<=>root.username;enabled:root.can-edit;}
                            }
                            VerticalBox {padding:0px;spacing:8px;
                                Text {text:root.tr("认证方式",root.english);color:Palette.foreground;}
                                ComboBox {model:[root.tr("密码",root.english),root.tr("私钥",root.english)];current-index<=>root.auth;enabled:root.can-edit;}
                            }
                        }
                        if root.auth==1:HorizontalBox {padding:0px;spacing:8px;
                            LineEdit {text<=>root.key-path;placeholder-text:root.tr("私钥文件",root.english);enabled:root.can-edit;}
                            Button {text:root.tr("选择私钥",root.english);enabled:root.can-edit;clicked=>{root.choose-key();}}
                        }
                        Text {text:root.tr(root.auth==0?"密码":"密钥口令",root.english);color:Palette.foreground;}
                        LineEdit {text<=>root.password;input-type:InputType.password;enabled:root.can-edit;placeholder-text:root.tr("已保存凭据时可留空",root.english);}
                        CheckBox {text:root.tr("记住凭据",root.english);checked<=>root.remember;enabled:root.can-edit;}
                        Button {text:root.tr("桌面连接设置",root.english);enabled:root.can-edit;clicked=>{root.advanced=!root.advanced;}}
                        if root.advanced:HorizontalBox {padding:0px;spacing:16px;
                            VerticalBox {padding:0px;spacing:8px;
                                Text {text:root.tr("显示编号",root.english);color:Palette.foreground;}
                                LineEdit {text<=>root.display;enabled:root.can-edit;}
                            }
                            VerticalBox {padding:0px;spacing:8px;
                                Text {text:root.tr("桌面授权文件",root.english);color:Palette.foreground;}
                                LineEdit {text<=>root.xauthority;enabled:root.can-edit;}
                            }
                        }
                        HorizontalBox {padding:0px;spacing:8px;
                            ActionButton {text:root.tr("连接设备",root.english);primary:true;enabled:root.can-edit;clicked=>{root.connect-device();}}
                            Button {text:root.tr("保存配置",root.english);enabled:root.can-edit;clicked=>{root.save-device();}}
                            Button {text:root.tr("移除设备配置",root.english);enabled:root.can-edit&&root.selected-id!="";clicked=>{root.remove-device();}}
                        }
                        Text {text:root.tr(root.error,root.english);color:#d34b4b;wrap:word-wrap;}
                        Text {text:root.connection-detail;color:Palette.foreground;wrap:word-wrap;}
                    }
                }
            }
            if root.page==1:ScrollView {
                viewport-height:settings.preferred-height;
                settings:=VerticalBox {padding:0px;spacing:16px;
                    Text {text:root.tr("通用",root.english);font-size:22px;font-weight:600;color:Palette.foreground;}
                    HorizontalBox {padding:0px;Text {text:root.tr("语言",root.english);color:Palette.foreground;}ComboBox {model:["简体中文","English"];current-index:root.english?1:0;enabled:root.can-interact;selected(_)=>{root.set-language(self.current-index);}}}
                    HorizontalBox {padding:0px;Text {text:root.tr("外观",root.english);color:Palette.foreground;}ComboBox {model:[root.tr("浅色",root.english),root.tr("深色",root.english),root.tr("跟随系统",root.english)];current-index:root.appearance;enabled:root.can-interact;selected(_)=>{root.set-appearance(self.current-index);}}}
                    HorizontalBox {padding:0px;Text {text:root.tr("主题色彩",root.english);color:Palette.foreground;}ComboBox {model:[root.tr("蓝色",root.english),root.tr("绿色",root.english),root.tr("石墨",root.english)];current-index:root.theme;enabled:root.can-interact;selected(_)=>{root.set-theme(self.current-index);}}}
                    HorizontalBox {padding:0px;Text {text:root.tr("桌面画质",root.english);color:Palette.foreground;}ComboBox {model:[root.tr("流畅",root.english),root.tr("清晰",root.english),root.tr("省资源",root.english)];current-index:root.desktop-mode;enabled:root.can-interact;selected(_)=>{root.set-desktop-mode(self.current-index);}}}
                    GlassSettings {english:root.english;enabled:root.glass-loaded&&root.can-interact&&!root.bg-busy;quality<=>root.glass-quality;material<=>root.bg-material;error:root.glass-error;tr(s,e)=>{return root.tr(s,e);}update(finished)=>{root.glass-controls(finished);}}
                    BackgroundSettings {english:root.english;enabled:root.can-interact&&!root.busy;busy:root.bg-busy;cancellable:root.bg-cancellable;
                        frame:root.bg-frame;has-frame:root.bg-has-frame;color:root.bg-color;source-width:root.bg-width;source-height:root.bg-height;
                        crop-x:root.bg-saved-crop-x;crop-y:root.bg-saved-crop-y;crop-width:root.bg-saved-crop-width;crop-height:root.bg-saved-crop-height;
                        hex<=>root.bg-hex;volume<=>root.bg-volume;status:root.bg-status;error:root.bg-error;
                        tr(s,e)=>{return root.tr(s,e);}action(s)=>{root.bg-action(s);}controls=>{root.bg-controls();}
                    }
                    Text {text:root.tr("存储",root.english);font-size:22px;font-weight:600;color:Palette.foreground;}
                    HorizontalBox {padding:0px;Text {text:root.tr("板端运行环境",root.english);color:Palette.foreground;}ComboBox {model:[root.tr("电脑下载后传输",root.english),root.tr("板端自行下载",root.english)];current-index:root.environment-source;enabled:root.can-interact;selected(_)=>{root.set-environment(self.current-index);}}}
                    HorizontalBox {padding:0px;Text {text:root.tr("日志上限（MiB）",root.english);color:Palette.foreground;}LineEdit {text<=>root.log-limit;enabled:root.can-interact;}Button {text:root.tr("保存",root.english);enabled:root.can-interact;clicked=>{root.save-log-limit(root.log-limit);}}}
                    HorizontalBox {padding:0px;
                        Text {text:root.tr("电脑本地日志目录",root.english);color:Palette.foreground;}
                        Button {text:root.tr("更换目录",root.english);enabled:!root.logs-mutating&&root.can-interact;clicked=>{root.choose-log-directory();}}
                        Button {text:root.tr("打开目录",root.english);enabled:!root.logs-mutating&&root.can-interact;clicked=>{root.open-log-directory(false);}}
                    }
                    Text {text:root.log-directory;color:Palette.foreground;wrap:word-wrap;}
                    Text {text:root.tr("资料位置",root.english);color:Palette.foreground;}
                    Text {text:root.data-directory;color:Palette.foreground;wrap:word-wrap;}
                    Text {text:root.tr(root.error,root.english);color:#d34b4b;wrap:word-wrap;}
                }
            }
            if root.page==3:VerticalBox {
                padding:0px;spacing:10px;
                HorizontalBox {padding:0px;spacing:8px;height:36px;vertical-stretch:0;
                    ComboBox {accessible-label:root.tr("日志项目",root.english);model:root.log-projects;current-index<=>root.log-project-index;enabled:root.can-browse;selected(_)=>{root.filter-logs(self.current-index,root.log-day-index,root.log-status-index);}}
                    ComboBox {accessible-label:root.tr("日志日期",root.english);model:root.log-days;current-index<=>root.log-day-index;enabled:root.can-browse;selected(_)=>{root.filter-logs(root.log-project-index,self.current-index,root.log-status-index);}}
                    ComboBox {accessible-label:root.tr("日志状态",root.english);model:[root.tr("全部状态",root.english),root.tr("记录中",root.english),root.tr("命令成功",root.english),root.tr("命令失败",root.english),root.tr("记录中断",root.english),root.tr("写入失败",root.english)];current-index<=>root.log-status-index;enabled:root.can-browse;selected(_)=>{root.filter-logs(root.log-project-index,root.log-day-index,self.current-index);}}
                    Button {text:root.tr(root.logs-loading?"正在读取…":"刷新",root.english);enabled:root.can-browse;clicked=>{root.refresh-logs();}}
                }
                if root.logs-notice!="":Text {text:root.logs-notice;color:Palette.foreground;wrap:word-wrap;vertical-stretch:0;}
                if root.logs-message!="":Text {text:root.logs-message;color:Palette.foreground;wrap:word-wrap;vertical-stretch:0;}
                if root.logs-error!="":Text {text:root.tr(root.logs-error,root.english);color:#d34b4b;wrap:word-wrap;vertical-stretch:0;}
                HorizontalBox {padding:0px;spacing:20px;
                    VerticalBox {padding:0px;spacing:8px;width:44%;
                        HorizontalBox {padding:0px;height:34px;vertical-stretch:0;
                            CheckBox {text:root.tr("选择本页",root.english);checked<=>root.logs-all-checked;enabled:root.can-browse&&root.logs-selectable;toggled=>{root.select-all-logs(self.checked);}}
                            Button {text:root.tr("清理所选",root.english)+" ("+root.logs-checked+")";enabled:root.can-browse&&!root.busy&&root.logs-checked>0;clicked=>{root.remove-logs();}}
                        }
                        if root.log-rows.length==0:Text {text:root.tr("暂无日志",root.english);color:Palette.foreground;horizontal-alignment:center;vertical-stretch:0;}
                        ListView {
                            for record in root.log-rows:HorizontalBox {padding:4px;spacing:6px;height:96px;
                                LogSelectionBox {accessible-label:root.tr("选择日志 "+record.command,root.english);selected:record.checked;enabled:root.can-browse&&record.removable;toggled=>{root.toggle-log(record.id,self.checked);}}
                                VerticalBox {padding:0px;spacing:3px;
                                    ActionButton {text:record.command;primary:record.selected;enabled:!root.logs-mutating&&root.can-interact;clicked=>{root.select-log(record.id);}}
                                    Text {text:record.context;color:Palette.foreground;overflow:elide;}
                                    Text {text:record.status;color:Palette.foreground;overflow:elide;}
                                }
                            }
                        }
                        HorizontalBox {padding:0px;height:34px;vertical-stretch:0;
                            Button {text:root.tr("上一页",root.english);enabled:root.can-browse&&root.logs-back;clicked=>{root.log-page(false);}}
                            Button {text:root.tr("下一页",root.english);enabled:root.can-browse&&root.logs-next;clicked=>{root.log-page(true);}}
                        }
                        Text {text:root.logs-summary;color:Palette.foreground;vertical-stretch:0;}
                    }
                    VerticalBox {padding:0px;spacing:8px;
                        if !root.log-selected:Text {text:root.tr("选择一份日志查看详情",root.english);color:Palette.foreground;vertical-stretch:0;}
                        if root.log-selected:VerticalBox {padding:0px;spacing:8px;
                            Text {text:root.log-command;color:Palette.foreground;font-weight:600;wrap:word-wrap;vertical-stretch:0;}
                            Text {text:root.log-metadata;color:Palette.foreground;wrap:word-wrap;vertical-stretch:0;}
                            if root.log-warning!="":Text {text:root.log-warning;color:Palette.foreground;wrap:word-wrap;vertical-stretch:0;}
                            if root.log-reading&&root.log-output=="":Text {text:root.tr("正在读取…",root.english);color:Palette.foreground;vertical-stretch:0;}
                            TextEdit {accessible-label:root.tr("日志原文",root.english);text:root.log-output;read-only:true;wrap:no-wrap;font-family:"Consolas";enabled:root.can-interact;}
                            Button {text:root.tr("打开日志目录",root.english);enabled:root.log-command!=""&&root.can-interact;clicked=>{root.open-log-directory(true);}}
                        }
                    }
                }
            }
            if root.page==7:ArenaView {
                english:root.english;enabled:root.can-interact;connected:root.connected;
                cameras:root.arena-cameras;camera-index<=>root.arena-camera-index;rate<=>root.arena-rate;
                camera-active:root.arena-camera-active;camera-loading:root.arena-camera-loading;frame:root.arena-frame;has-frame:root.arena-has-frame;
                camera-status:root.arena-camera-status;metrics-status:root.arena-metrics-status;metrics:root.arena-metrics;cpu-path:root.arena-cpu-path;memory-path:root.arena-memory-path;
                live:root.arena-pose-live;world-x:root.arena-world-x;world-y:root.arena-world-y;heading:root.arena-heading;has-heading:root.arena-has-heading;pose-status:root.arena-pose-status;coordinates:root.arena-coordinates;
                tr(s,e)=>{return root.tr(s,e);}toggle-camera=>{root.arena-toggle-camera();}refresh-cameras=>{root.arena-refresh-cameras();}
            }
            if root.page==6:FilesView {
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
                english:root.english;enabled:root.can-interact;connected:root.connected;active:root.desktop-active;wanted:root.desktop-wanted;ready:root.desktop-ready;
                mode:root.desktop-mode;fit<=>root.desktop-fit;read-only<=>root.desktop-readonly;frame:root.desktop-frame;source-width:root.desktop-source-width;source-height:root.desktop-source-height;
                cursor:root.desktop-cursor;cursor-known:root.desktop-cursor-known;cursor-width:root.desktop-cursor-width;cursor-height:root.desktop-cursor-height;cursor-hot-x:root.desktop-cursor-hot-x;cursor-hot-y:root.desktop-cursor-hot-y;
                status:root.desktop-status;error:root.desktop-error;clipboard-available:root.desktop-clipboard-available;focus-revision:root.desktop-focus-revision;
                tr(s,e)=>{return root.tr(s,e);}action(name)=>{root.desktop-action(name);}quality(index)=>{root.set-desktop-mode(index);}readonly-changed=>{root.desktop-readonly-changed();}reboot=>{root.system-action("reboot");}
                pointer(x,y,w,h,b,d)=>{root.desktop-pointer(x,y,w,h,b,d);}motion(x,y,w,h)=>{root.desktop-motion(x,y,w,h);}wheel(x,y,w,h,b,n)=>{root.desktop-wheel(x,y,w,h,b,n);}
                key(value,ctrl,alt,shift)=>{return root.desktop-key(value,ctrl,alt,shift);}text(value)=>{root.desktop-text(value);}focused(value)=>{root.desktop-focused(value);}
            }
            if root.page==4:TerminalView {
                tabs:root.terminal-tabs;active:root.terminal-active;cells:root.terminal-cells;
                cursor-column:root.terminal-cursor-column;cursor-row:root.terminal-cursor-row;cursor-visible:root.terminal-cursor-visible;
                phase:root.terminal-phase;status:root.terminal-status;error:root.terminal-error;log-status:root.terminal-log-status;
                connected:root.connected;busy:root.terminal-busy;sending:root.terminal-sending;enabled:root.can-interact;english:root.english;
                draft<=>root.terminal-draft;project<=>root.terminal-project;
                completions:root.terminal-completions;completion-index:root.terminal-completion-index;
                completion-loading:root.terminal-completion-loading;completion-truncated:root.terminal-completion-truncated;
                command-caret:root.terminal-command-caret;command-revision:root.terminal-command-revision;
                tr(s,e)=>{return root.tr(s,e);}
                select-tab(key)=>{root.terminal-select(key);}action(name)=>{root.terminal-action(name);}
                copy=>{root.terminal-copy();}paste=>{root.terminal-paste();}scroll(n)=>{root.terminal-scroll(n);}
                select(x,y,x2,y2)=>{root.terminal-selection(x,y,x2,y2);}resize-grid(x,y)=>{root.terminal-resize(x,y);}
                committed(value)=>{root.terminal-committed(value);}key(value,ctrl,alt,shift)=>{return root.terminal-key(value,ctrl,alt,shift);}
                draft-edited(value,cursor)=>{root.terminal-draft-edited(value,cursor);}project-edited(value)=>{root.terminal-project-edited(value);}
                editor-key(value,ctrl,shift,alt,expanded)=>{return root.terminal-editor-key(value,ctrl,shift,alt,expanded);}
                editor-focused(value)=>{root.terminal-editor-focused(value);}choose-completion(index)=>{root.terminal-choose-completion(index);}
                send-command=>{root.terminal-send-command();}
            }
            if root.page==2:VerticalBox {padding:0px;Text {text:"PortCanvas · v0.6.0 dev";font-size:22px;color:Palette.foreground;}HorizontalBox {padding:0px;height:36px;vertical-stretch:0;Button {text:root.tr("打开仓库",root.english);clicked=>{root.system-action("repository");}}Button {text:root.tr("打开安装目录",root.english);clicked=>{root.system-action("directory");}}}AboutSlint {preferred-height:120px;vertical-stretch:0;}}
        }}
        Rectangle {
            visible:root.dialog-kind!=0;
            background:#00000070;
            TouchArea {}
            Rectangle {
                width:root.dialog-kind==8?root.width - 64px:min(root.width - 64px,640px);height:root.dialog-kind==8?root.height - 80px:panel.preferred-height;
                x:(root.width - self.width)/2;y:(root.height - self.height)/2;
                background:Palette.background;border-radius:10px;border-width:1px;border-color:Palette.border;
                modal-focus:=FocusScope {
                    key-pressed(event)=>{if root.dialog-kind==8{if event.text==Key.Escape{root.bg-crop-response(false);return accept;}return reject;}if event.text==Key.Escape{root.respond(false);return accept;}if event.text==Key.Tab{root.accept-focused=!root.accept-focused;return accept;}if event.text==Key.Return{root.respond(root.accept-focused);return accept;}return accept;}
                    crop-view:=BackgroundCropView {visible:root.dialog-kind==8;english:root.english;busy:root.bg-busy;frame:root.bg-preview;
                        source-width:root.bg-preview-width;source-height:root.bg-preview-height;crop-x:root.bg-crop-x;crop-y:root.bg-crop-y;crop-width:root.bg-crop-width;crop-height:root.bg-crop-height;error:root.bg-error;
                        tr(s,e)=>{return root.tr(s,e);}response(v)=>{root.bg-crop-response(v);}reset=>{root.bg-crop-reset();}gesture(k,x,y)=>{root.bg-crop-gesture(k,x,y);}arrow(x,y,r)=>{root.bg-crop-arrow(x,y,r);}
                    }
                    panel:=VerticalBox {visible:root.dialog-kind!=8;padding:24px;spacing:18px;
                        Text {text:root.tr(root.dialog-title,root.english);font-size:20px;font-weight:600;color:Palette.foreground;}
                        Text {text:root.migration-executing?root.tr("正在迁移资料…",root.english):root.system-executing?root.tr("正在提交重启请求…",root.english):root.dialog-message;wrap:word-wrap;color:Palette.foreground;}
                        HorizontalBox {padding:0px;
                            ActionButton {enabled:!root.system-executing&&!root.migration-executing;text:root.tr("取消",root.english);primary:!root.accept-focused;clicked=>{root.respond(false);}}
                            ActionButton {enabled:!root.system-executing&&!root.migration-executing;text:root.tr(root.dialog-kind==2?"信任并连接":root.dialog-kind==3?"清理所选":(root.dialog-kind==4||root.dialog-kind==9)?"确认重启":root.dialog-kind==5?"确认关闭":root.dialog-kind==6?"确认覆盖":root.dialog-kind==7?"确认删除":root.dialog-kind==10?"确认导入":root.dialog-kind==11?"确认恢复":"确认移除",root.english);primary:root.accept-focused;clicked=>{root.respond(true);}}
                        }
                    }
                }
            }
        }
    }
}
