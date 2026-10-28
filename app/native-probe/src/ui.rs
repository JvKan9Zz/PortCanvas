slint::slint! {
    import {Button,LineEdit,VerticalBox,HorizontalBox,AboutSlint,Palette} from "std-widgets.slint";
    export struct TerminalCell { column:int,row:int,text:string,ink:color,paper:color,wide:bool,bold:bool,underline:bool }
    export component ProbeWindow inherits Window {
        title:"PortCanvas · Native probe";
        preferred-width:1000px;preferred-height:720px;min-width:860px;min-height:600px;
        default-font-family:"Microsoft YaHei UI";default-font-size:14px;
        background:#f3f5f8;
        init=>{Palette.color-scheme=ColorScheme.light;}
        in property <[TerminalCell]> cells;
        in property <int> cursor-column;
        in property <int> cursor-row;
        in property <image> desktop;
        in property <image> video;
        in property <string> status:"就绪";
        in-out property <int> page:0;
        in-out property <string> draft;
        in property <string> selection;
        in-out property <bool> cover:false;
        callback sample(); callback alternate(); callback scroll(int);
        callback key(string,bool,bool); callback submit(string);
        callback select(int,int,int,int); callback copy(); callback resize-grid(int,int);
        callback start-desktop(); callback start-video(); callback stop();
        callback pointer(int,int,int); callback desktop-key(string,bool);
        callback page-changed(int);
        changed page => {root.page-changed(root.page);}
        VerticalBox {
            padding:20px;spacing:16px;
            HorizontalBox {
                height:36px;vertical-stretch:0;padding:0px;
                Text {text:"PortCanvas";font-size:22px;font-weight:650;color:#1c293b;}
                Text {text:"本机原生验证";color:#526174;horizontal-alignment:right;}
            }
            HorizontalBox {
                height:40px;vertical-stretch:0;padding:0px;
                Button {text:"终端";primary:root.page==0;clicked=>{root.page=0;}}
                Button {text:"本机桌面";primary:root.page==1;clicked=>{root.page=1;}}
                Button {text:"视频";primary:root.page==2;clicked=>{root.page=2;}}
                Button {text:"关于";primary:root.page==3;clicked=>{root.page=3;}}
            }
            if root.page==0: VerticalBox {
                padding:0px;spacing:12px;vertical-stretch:1;
                HorizontalBox {
                    height:36px;vertical-stretch:0;padding:0px;
                    Button {text:"加载样例";clicked=>{root.sample();}}
                    Button {text:"切换备用屏幕";clicked=>{root.alternate();}}
                    Button {text:"向上滚动";clicked=>{root.scroll(5);}}
                    Button {text:"向下滚动";clicked=>{root.scroll(-5);}}
                    Button {text:"复制选区";clicked=>{root.copy();}}
                }
                grid := Rectangle {
                    background:#181c24;border-radius:6px;clip:true;
                    property <int> columns:floor((self.width - 16px)/9px);
                    property <int> rows:floor((self.height - 16px)/21px);
                    changed columns=>{root.resize-grid(grid.columns,grid.rows);}
                    changed rows=>{root.resize-grid(grid.columns,grid.rows);}
                    for cell in root.cells: Rectangle {
                        x:8px+cell.column*9px;y:8px+cell.row*21px;
                        width:cell.wide?18px:9px;height:21px;background:cell.paper;
                        Text {text:cell.text;color:cell.ink;font-family:"Consolas";font-size:14px;font-weight:cell.bold?700:400;vertical-alignment:center;}
                        if cell.underline: Rectangle {y:19px;height:1px;background:cell.ink;}
                    }
                    Rectangle {x:8px+root.cursor-column*9px;y:8px+root.cursor-row*21px;width:9px;height:21px;border-width:1px;border-color:#d8dfeb;}
                    focus := FocusScope {
                        key-pressed(event)=>{root.key(event.text,event.modifiers.control,event.modifiers.shift);return accept;}
                        touch := TouchArea {
                            property <int> start-x;property <int> start-y;
                            pointer-event(event)=>{
                                if event.kind==PointerEventKind.down {
                                    self.start-x=max(0,floor((self.mouse-x - 8px)/9px));self.start-y=max(0,floor((self.mouse-y - 8px)/21px));focus.focus();
                                }
                                if event.kind==PointerEventKind.up {root.select(self.start-x,self.start-y,max(0,floor((self.mouse-x - 8px)/9px)),max(0,floor((self.mouse-y - 8px)/21px)));}
                            }
                            scroll-event(event)=>{root.scroll(event.delta-y>0px?3:-3);return accept;}
                        }
                    }
                }
                HorizontalBox {
                    editor := LineEdit {text<=>root.draft;placeholder-text:"输入文字后回显";accepted=>{root.submit(root.draft);root.draft="";}}
                    Button {text:"回显";clicked=>{root.submit(root.draft);root.draft="";}}
                }
            }
            if root.page==1: VerticalBox {
                padding:0px;spacing:12px;vertical-stretch:1;
                HorizontalBox {
                    height:36px;vertical-stretch:0;padding:0px;
                    Button {text:"连接本机图样";clicked=>{root.start-desktop();}}
                    Button {text:"断开";clicked=>{root.stop();}}
                }
                Rectangle {
                    background:#181c24;border-radius:6px;vertical-stretch:1;min-height:200px;
                    // The fixture is 16:9; match the input region to the displayed image.
                    picture := Image {source:root.desktop;image-fit:contain;width:min(parent.width,parent.height*16/9);height:self.width*9/16;x:(parent.width - self.width)/2;y:(parent.height - self.height)/2;
                        remote-focus:=FocusScope {
                            key-pressed(event)=>{root.desktop-key(event.text,true);return accept;}
                            key-released(event)=>{root.desktop-key(event.text,false);return accept;}
                            mouse:=TouchArea {
                                moved=>{root.pointer(round(self.mouse-x/self.width*319),round(self.mouse-y/self.height*179),self.pressed?1:0);}
                                pointer-event(event)=>{remote-focus.focus();root.pointer(round(self.mouse-x/self.width*319),round(self.mouse-y/self.height*179),event.kind==PointerEventKind.down?1:0);}
                            }
                        }
                    }
                }
            }
            if root.page==2: VerticalBox {
                padding:0px;spacing:12px;vertical-stretch:1;
                HorizontalBox {
                    height:36px;vertical-stretch:0;padding:0px;
                    Button {text:"播放测试视频";clicked=>{root.start-video();}}
                    Button {text:"停止";clicked=>{root.stop();}}
                    Button {text:root.cover?"完整画面":"填充裁剪";clicked=>{root.cover=!root.cover;}}
                }
                Rectangle {background:#181c24;border-radius:6px;clip:true;vertical-stretch:1;min-height:200px;Image {width:100%;height:100%;source:root.video;image-fit:root.cover ? ImageFit.cover : ImageFit.contain;}}
            }
            if root.page==3: VerticalBox {Text {text:"PortCanvas v0.6.0 原生链路原型";color:#1c293b;font-size:20px;}AboutSlint {}}
            Text {text:root.status;color:#43546b;wrap:word-wrap;vertical-stretch:0;}
        }
    }
}
