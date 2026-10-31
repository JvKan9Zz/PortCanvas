slint::slint! {
    import {Button,LineEdit,ComboBox,CheckBox,ScrollView,ListView,TextEdit,VerticalBox,HorizontalBox,Palette,AboutSlint} from "std-widgets.slint";
    import {TerminalView,TerminalCell,TerminalTab,CompletionRow} from "terminal_view.slint";
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
        property <bool> can-browse:!root.logs-loading&&!root.logs-mutating&&root.dialog-kind==0;
        callback refresh-logs();callback poll-logs();callback filter-logs(int,int,int);callback log-page(bool);
        callback select-log(string);callback toggle-log(string,bool);callback select-all-logs(bool);callback remove-logs();
        callback choose-log-directory();callback open-log-directory(bool);callback fullscreen();callback set-theme(int);
        changed theme=>{AppColors.accent=root.theme==1?#2b8169:root.theme==2?#53657f:#356ce5;}
        changed page=>{root.terminal-cancel-completion();if root.page==3{root.refresh-logs();}if root.page==4{root.terminal-reload();}}
        changed native-hidden=>{if root.native-hidden{root.terminal-cancel-completion();}if !root.native-hidden&&root.page==3{root.refresh-logs();}if !root.native-hidden&&root.page==4{root.terminal-reload();}}
        Timer {interval:1200ms;running:root.page==3&&root.log-live&&!root.native-hidden&&root.dialog-kind==0;triggered=>{root.poll-logs();}}

        in-out property <int> environment-source:0;
        in-out property <int> desktop-mode:0;
        in-out property <string> log-limit:"1024";
        in-out property <int> dialog-kind:0;
        in property <string> dialog-title;
        in property <string> dialog-message;
        property <bool> accept-focused:false;
        property <bool> can-edit:!root.busy && !root.connected && !root.logs-mutating && root.dialog-kind==0;
        callback select-device(string);callback select-found(string);callback new-device();
        callback save-device();callback connect-device();callback disconnect-device();callback remove-device();
        callback choose-key();callback discover();callback cancel-discovery();callback respond(bool);
        callback set-language(int);callback set-appearance(int);callback set-environment(int);callback set-desktop-mode(int);callback save-log-limit(string);
        changed appearance=>{Palette.color-scheme=root.appearance==0?ColorScheme.light:root.appearance==1?ColorScheme.dark:ColorScheme.unknown;}
        changed dialog-kind=>{if root.dialog-kind!=0 {root.accept-focused=false;modal-focus.focus();}else{main-focus.focus();}}
        main-focus:=FocusScope { VerticalBox {
            padding:20px;spacing:16px;
            HorizontalBox {
                padding:0px;height:40px;vertical-stretch:0;
                Text {text:"PortCanvas";color:Palette.foreground;font-size:25px;font-weight:650;}
                Button {text:root.tr(root.window-is-fullscreen?"退出全屏":"全屏",root.english);horizontal-stretch:0;preferred-width:110px;enabled:root.dialog-kind==0;clicked=>{root.fullscreen();}}
                Text {text:root.tr(root.connection-status,root.english);color:Palette.foreground;horizontal-alignment:right;vertical-alignment:center;}
                Button {text:root.tr(root.busy?"取消连接":"断开连接",root.english);visible:root.busy||root.connected;enabled:root.dialog-kind==0;clicked=>{root.disconnect-device();}}
            }
            HorizontalBox {
                padding:0px;height:38px;vertical-stretch:0;
                ActionButton {text:root.tr("设备连接与管理",root.english);primary:root.page==0;enabled:root.dialog-kind==0;clicked=>{root.page=0;}}
                ActionButton {text:root.tr("终端",root.english);primary:root.page==4;enabled:root.dialog-kind==0;clicked=>{root.page=4;}}
                ActionButton {text:root.tr("日志",root.english);primary:root.page==3;enabled:root.dialog-kind==0;clicked=>{root.page=3;}}
                ActionButton {text:root.tr("设置",root.english);primary:root.page==1;enabled:root.dialog-kind==0;clicked=>{root.page=1;}}
                ActionButton {text:root.tr("关于",root.english);primary:root.page==2;enabled:root.dialog-kind==0;clicked=>{root.page=2;}}
            }
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
                        Button {text:root.tr(root.discovering?"停止搜索":"搜索",root.english);enabled:!root.busy&&root.dialog-kind==0;clicked=>{if root.discovering{root.cancel-discovery();}else{root.discover();}}}
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
                    HorizontalBox {padding:0px;Text {text:root.tr("语言",root.english);color:Palette.foreground;}ComboBox {model:["简体中文","English"];current-index:root.english?1:0;enabled:root.dialog-kind==0;selected(_)=>{root.set-language(self.current-index);}}}
                    HorizontalBox {padding:0px;Text {text:root.tr("外观",root.english);color:Palette.foreground;}ComboBox {model:[root.tr("浅色",root.english),root.tr("深色",root.english),root.tr("跟随系统",root.english)];current-index:root.appearance;enabled:root.dialog-kind==0;selected(_)=>{root.set-appearance(self.current-index);}}}
                    HorizontalBox {padding:0px;Text {text:root.tr("主题色彩",root.english);color:Palette.foreground;}ComboBox {model:[root.tr("蓝色",root.english),root.tr("绿色",root.english),root.tr("石墨",root.english)];current-index:root.theme;enabled:root.dialog-kind==0;selected(_)=>{root.set-theme(self.current-index);}}}
                    HorizontalBox {padding:0px;Text {text:root.tr("桌面画质",root.english);color:Palette.foreground;}ComboBox {model:[root.tr("流畅",root.english),root.tr("清晰",root.english),root.tr("省资源",root.english)];current-index:root.desktop-mode;enabled:root.dialog-kind==0;selected(_)=>{root.set-desktop-mode(self.current-index);}}}
                    Text {text:root.tr("存储",root.english);font-size:22px;font-weight:600;color:Palette.foreground;}
                    HorizontalBox {padding:0px;Text {text:root.tr("板端运行环境",root.english);color:Palette.foreground;}ComboBox {model:[root.tr("电脑下载后传输",root.english),root.tr("板端自行下载",root.english)];current-index:root.environment-source;enabled:root.dialog-kind==0;selected(_)=>{root.set-environment(self.current-index);}}}
                    HorizontalBox {padding:0px;Text {text:root.tr("日志上限（MiB）",root.english);color:Palette.foreground;}LineEdit {text<=>root.log-limit;enabled:root.dialog-kind==0;}Button {text:root.tr("保存",root.english);enabled:root.dialog-kind==0;clicked=>{root.save-log-limit(root.log-limit);}}}
                    HorizontalBox {padding:0px;
                        Text {text:root.tr("电脑本地日志目录",root.english);color:Palette.foreground;}
                        Button {text:root.tr("更换目录",root.english);enabled:!root.logs-mutating&&root.dialog-kind==0;clicked=>{root.choose-log-directory();}}
                        Button {text:root.tr("打开目录",root.english);enabled:!root.logs-mutating&&root.dialog-kind==0;clicked=>{root.open-log-directory(false);}}
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
                                    ActionButton {text:record.command;primary:record.selected;enabled:!root.logs-mutating&&root.dialog-kind==0;clicked=>{root.select-log(record.id);}}
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
                            TextEdit {accessible-label:root.tr("日志原文",root.english);text:root.log-output;read-only:true;wrap:no-wrap;font-family:"Consolas";enabled:root.dialog-kind==0;}
                            Button {text:root.tr("打开日志目录",root.english);enabled:root.log-command!=""&&root.dialog-kind==0;clicked=>{root.open-log-directory(true);}}
                        }
                    }
                }
            }
            if root.page==4:TerminalView {
                tabs:root.terminal-tabs;active:root.terminal-active;cells:root.terminal-cells;
                cursor-column:root.terminal-cursor-column;cursor-row:root.terminal-cursor-row;cursor-visible:root.terminal-cursor-visible;
                phase:root.terminal-phase;status:root.terminal-status;error:root.terminal-error;log-status:root.terminal-log-status;
                connected:root.connected;busy:root.terminal-busy;sending:root.terminal-sending;enabled:root.dialog-kind==0;english:root.english;
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
            if root.page==2:VerticalBox {padding:0px;Text {text:"PortCanvas · v0.6.0 dev";font-size:22px;color:Palette.foreground;}AboutSlint {preferred-height:120px;vertical-stretch:0;}}
        }}
        Rectangle {
            visible:root.dialog-kind!=0;
            background:#00000070;
            TouchArea {}
            Rectangle {
                width:min(root.width - 64px,640px);height:panel.preferred-height;
                x:(root.width - self.width)/2;y:(root.height - self.height)/2;
                background:Palette.background;border-radius:10px;border-width:1px;border-color:Palette.border;
                modal-focus:=FocusScope {
                    key-pressed(event)=>{if event.text==Key.Escape{root.respond(false);return accept;}if event.text==Key.Tab{root.accept-focused=!root.accept-focused;return accept;}if event.text==Key.Return{root.respond(root.accept-focused);return accept;}return accept;}
                    panel:=VerticalBox {padding:24px;spacing:18px;
                        Text {text:root.tr(root.dialog-title,root.english);font-size:20px;font-weight:600;color:Palette.foreground;}
                        Text {text:root.dialog-message;wrap:word-wrap;color:Palette.foreground;}
                        HorizontalBox {padding:0px;
                            ActionButton {text:root.tr("取消",root.english);primary:!root.accept-focused;clicked=>{root.respond(false);}}
                            ActionButton {text:root.tr(root.dialog-kind==2?"信任并连接":root.dialog-kind==3?"清理所选":root.dialog-kind==4?"确认重启":root.dialog-kind==5?"确认关闭":"确认移除",root.english);primary:root.accept-focused;clicked=>{root.respond(true);}}
                        }
                    }
                }
            }
        }
    }
}
