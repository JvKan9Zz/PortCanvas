use crate::{workbench::text, workbench_ui::Workbench};
use anyhow::{ensure, Context, Result};
use portcanvas_native_probe::update_client::{Channel, Plan};
use slint::ComponentHandle;
use std::{path::PathBuf, rc::Rc, sync::{Arc, Mutex}, time::Instant};
use tokio::{runtime::Runtime, task::JoinHandle};
use tokio_util::sync::CancellationToken;

const KEY: &str = include_str!("../assets/update-public-key.txt");
#[derive(Default)]
struct State {
    phase: String, message: String, plan: Option<Plan>, bytes: Option<Vec<u8>>,
    downloaded: u64, total: Option<u64>, cancel: CancellationToken,
}
pub struct Controller {
    ui: slint::Weak<Workbench>, runtime: Arc<Runtime>,
    jobs: Arc<Mutex<Vec<JoinHandle<()>>>>, state: Arc<Mutex<State>>,
    channel: Channel,
}
impl Controller {
    pub fn install(ui:&Workbench,runtime:Arc<Runtime>,jobs:Arc<Mutex<Vec<JoinHandle<()>>>>)->Result<Rc<Self>>{
        #[cfg(debug_assertions)] let channel=Channel::from_environment(KEY)?;
        #[cfg(not(debug_assertions))] let channel=Channel::beta(KEY);
        let c=Rc::new(Self{ui:ui.as_weak(),runtime,jobs,state:Arc::new(Mutex::new(State::default())),channel});
        ui.on_update_action({let c=c.clone();move|name|c.action(name.as_str())});
        ui.on_update_changed({let c=c.clone();move||c.render()});
        c.render();Ok(c)
    }
    fn render(&self){
        let Some(ui)=self.ui.upgrade() else{return};let s=self.state.lock().unwrap();
        ui.set_update_phase(s.phase.as_str().into());
        ui.set_update_message(text(&s.message,ui.get_english()).into());
        ui.set_update_version(s.plan.as_ref().map(|p|p.version.as_str()).unwrap_or_default().into());
        ui.set_update_notes(s.plan.as_ref().map(|p|p.notes.as_str()).unwrap_or_default().into());
        ui.set_update_busy(matches!(s.phase.as_str(),"checking"|"downloading"));
        ui.set_update_progress(s.total.filter(|v|*v>0).map(|v|s.downloaded as f32/v as f32).unwrap_or(0.));
    }
    fn action(&self,name:&str){
        let Some(ui)=self.ui.upgrade() else{return};
        if name=="cancel"{self.state.lock().unwrap().cancel.cancel();return;}
        if name=="install"{
            if ui.get_dialog_kind()!=0||ui.get_file_mutating()||ui.get_migration_busy()||ui.get_system_busy()||ui.get_busy(){return;}
            if self.state.lock().unwrap().phase!="ready"{return;}
            ui.set_dialog_title(text("安装更新",ui.get_english()).into());
            ui.set_dialog_message(text("安装完成后将重新打开预览版；本机连接会断开，远端持久终端可在重连后恢复。",ui.get_english()).into());
            ui.set_dialog_kind(13);return;
        }
        if !matches!(name,"check"|"download"){return;}
        let downloading=name=="download";
        let (cancel,plan)={let mut s=self.state.lock().unwrap();
            if matches!(s.phase.as_str(),"checking"|"downloading"){return;}
            if downloading&&s.plan.is_none(){return;}
            s.cancel=CancellationToken::new();s.bytes=None;s.message.clear();s.downloaded=0;s.total=None;
            s.phase=if downloading{"downloading"}else{"checking"}.into();
            if !downloading{s.plan=None;}
            (s.cancel.clone(),s.plan.clone())};
        self.render();let state=self.state.clone();let weak=self.ui.clone();let channel=self.channel.clone();
        self.jobs.lock().unwrap().push(self.runtime.spawn(async move{
            let result:Result<()>=async{
                if let Some(plan)=plan.filter(|_|downloading){
                    let progress_state=state.clone();let progress_ui=weak.clone();let last=Mutex::new(Instant::now());
                    let bytes=channel.download(&plan,env!("CARGO_PKG_VERSION"),&cancel,move|chunk,total,_|{
                        {let mut s=progress_state.lock().unwrap();s.downloaded+=chunk as u64;s.total=total;}
                        let mut last=last.lock().unwrap();if last.elapsed().as_millis()>=100{
                            *last=Instant::now();let _=progress_ui.upgrade_in_event_loop(|ui|ui.invoke_update_changed());
                        }
                    }).await?;
                    let mut s=state.lock().unwrap();s.bytes=Some(bytes);s.phase="ready".into();
                }else{
                    let plan=channel.check(env!("CARGO_PKG_VERSION"),&cancel).await?;
                    let mut s=state.lock().unwrap();s.phase=if plan.is_some(){"available"}else{"current"}.into();s.plan=plan;
                }Ok(())
            }.await;
            if let Err(error)=result{let mut s=state.lock().unwrap();s.phase=if cancel.is_cancelled(){"canceled"}else{"error"}.into();s.message=error.to_string();}
            let _=weak.upgrade_in_event_loop(|ui|ui.invoke_update_changed());
        }));
    }
    pub fn respond(&self,accepted:bool){
        let Some(ui)=self.ui.upgrade() else{return};
        ui.set_dialog_kind(0);if !accepted{return;}
        let result=self.begin_install(&ui);
        if let Err(error)=result{let mut s=self.state.lock().unwrap();s.phase="error".into();s.message=error.to_string();drop(s);self.render();}
    }
    fn begin_install(&self,ui:&Workbench)->Result<()>{
        ensure!(!ui.get_file_mutating()&&!ui.get_migration_busy()&&!ui.get_system_busy()&&!ui.get_busy(),"请先完成正在进行的操作。");
        let s=self.state.lock().unwrap();ensure!(s.phase=="ready","请先下载并校验更新。");
        let plan=s.plan.as_ref().context("请重新检查更新。")?;let bytes=s.bytes.as_ref().context("请重新下载更新。")?;
        portcanvas_native_probe::update::verify(bytes,&self.channel.public_key,&plan.signature,env!("CARGO_PKG_VERSION"),&plan.version)?;
        let dir=PathBuf::from(ui.get_data_directory().as_str()).join("updates");std::fs::create_dir_all(&dir)?;
        let package=dir.join("PortCanvasBeta-update.exe");std::fs::write(&package,bytes)?;
        let readback=std::fs::read(&package)?;
        portcanvas_native_probe::update::verify(&readback,&self.channel.public_key,&plan.signature,env!("CARGO_PKG_VERSION"),&plan.version)?;
        let exe=std::env::current_exe()?;ensure!(exe.file_name().is_some_and(|name|name=="PortCanvasBeta.exe"),"此开发构建请使用预览版安装器进行安装。");let install_dir=exe.parent().context("无法确定安装位置。")?;
        let mut command=std::process::Command::new(package);
        command.arg("/S").arg(format!("/PID={}",std::process::id())).arg("/RESTART");
        #[cfg(windows)] {use std::os::windows::process::CommandExt;command.raw_arg(format!("/D={}",install_dir.display()));}
        command.spawn().context("无法启动更新安装器。")?;
        slint::quit_event_loop()?;Ok(())
    }
    pub fn shutdown(&self){self.state.lock().unwrap().cancel.cancel();}
}
