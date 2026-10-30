slint::slint! {
    import {Button,LineEdit,ComboBox,CheckBox,ScrollView,VerticalBox,HorizontalBox,Palette,AboutSlint} from "std-widgets.slint";
    export struct DeviceRow {id:string,name:string,address:string}
    export component Workbench inherits Window {
        title:"PortCanvas · Demo V0.6.0 (dev)";
        preferred-width:1200px;preferred-height:820px;min-width:1000px;min-height:700px;
        default-font-family:"Microsoft YaHei UI";default-font-size:14px;
        background:Palette.background;
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
        in-out property <int> environment-source:0;
        in-out property <int> desktop-mode:0;
        in-out property <string> log-limit:"1024";
        in-out property <int> dialog-kind:0;
        in property <string> dialog-title;
        in property <string> dialog-message;
        property <bool> accept-focused:false;
        property <bool> can-edit:!root.busy && !root.connected && root.dialog-kind==0;
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
                Text {text:root.tr(root.connection-status,root.english);color:Palette.foreground;horizontal-alignment:right;vertical-alignment:center;}
                Button {text:root.tr(root.busy?"取消连接":"断开连接",root.english);visible:root.busy||root.connected;enabled:root.dialog-kind==0;clicked=>{root.disconnect-device();}}
            }
            HorizontalBox {
                padding:0px;height:38px;vertical-stretch:0;
                Button {text:root.tr("设备连接与管理",root.english);primary:root.page==0;enabled:root.dialog-kind==0;clicked=>{root.page=0;}}
                Button {text:root.tr("设置",root.english);primary:root.page==1;enabled:root.dialog-kind==0;clicked=>{root.page=1;}}
                Button {text:root.tr("关于",root.english);primary:root.page==2;enabled:root.dialog-kind==0;clicked=>{root.page=2;}}
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
                            for device in root.devices: Button {text:device.name+"\n"+device.address;primary:root.selected-id==device.id;enabled:root.can-edit;preferred-height:58px;vertical-stretch:0;clicked=>{root.select-device(device.id);}}
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
                            Button {text:root.tr("连接设备",root.english);primary:true;enabled:root.can-edit;clicked=>{root.connect-device();}}
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
                    HorizontalBox {padding:0px;Text {text:root.tr("桌面画质",root.english);color:Palette.foreground;}ComboBox {model:[root.tr("流畅",root.english),root.tr("清晰",root.english),root.tr("省资源",root.english)];current-index:root.desktop-mode;enabled:root.dialog-kind==0;selected(_)=>{root.set-desktop-mode(self.current-index);}}}
                    Text {text:root.tr("存储",root.english);font-size:22px;font-weight:600;color:Palette.foreground;}
                    HorizontalBox {padding:0px;Text {text:root.tr("板端运行环境",root.english);color:Palette.foreground;}ComboBox {model:[root.tr("电脑下载后传输",root.english),root.tr("板端自行下载",root.english)];current-index:root.environment-source;enabled:root.dialog-kind==0;selected(_)=>{root.set-environment(self.current-index);}}}
                    HorizontalBox {padding:0px;Text {text:root.tr("日志上限（MiB）",root.english);color:Palette.foreground;}LineEdit {text<=>root.log-limit;enabled:root.dialog-kind==0;}Button {text:root.tr("保存",root.english);enabled:root.dialog-kind==0;clicked=>{root.save-log-limit(root.log-limit);}}}
                    Text {text:root.tr("资料位置",root.english);color:Palette.foreground;}
                    Text {text:root.data-directory;color:Palette.foreground;wrap:word-wrap;}
                    Text {text:root.tr(root.error,root.english);color:#d34b4b;wrap:word-wrap;}
                }
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
                            Button {text:root.tr("取消",root.english);primary:!root.accept-focused;clicked=>{root.respond(false);}}
                            Button {text:root.tr(root.dialog-kind==2?"信任并连接":"确认移除",root.english);primary:root.accept-focused;clicked=>{root.respond(true);}}
                        }
                    }
                }
            }
        }
    }
}
