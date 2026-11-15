use anyhow::{ensure,Result};
use serde::{Deserialize,Serialize};
#[derive(Clone,Default,Deserialize,Serialize)]
#[serde(default,rename_all="camelCase")]
pub struct ProgramLaunch {pub path:String,pub directory:String,pub interpreter:u8,pub sudo:bool}
fn quote(value:&str)->String{format!("'{}'",value.replace('\'',"'\"'\"'"))}
impl ProgramLaunch {
    pub fn command(&self)->Result<String>{
        ensure!(self.path.starts_with('/')&&!self.path.ends_with('/')&&!self.path.chars().any(char::is_control),"请输入板端程序的完整绝对路径。");
        ensure!(self.interpreter<=3,"程序运行方式无效。");
        let directory=if self.directory.is_empty(){self.path.rsplit_once('/').unwrap().0}else{&self.directory};
        let directory=if directory.is_empty(){"/"}else{directory};
        ensure!(directory.starts_with('/')&&!directory.chars().any(char::is_control),"请输入有效的板端运行目录。");
        let mode=if self.interpreter==0{if self.path.ends_with(".py")||self.path.ends_with(".pyc"){1}else if self.path.ends_with(".sh"){2}else{3}}else{self.interpreter};
        Ok(format!("cd -- {} && {}{}{}",quote(directory),if self.sudo{"sudo -- "}else{""},match mode{1=>"python3 -u -- ",2=>"bash -- ",_=>""},quote(&self.path)))
    }
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn paths_are_literal_and_programs_are_manual_commands(){
        let p=ProgramLaunch{path:"/home/bcsh/中文 ';$(touch nope).py".into(),sudo:true,..Default::default()};
        let command=p.command().unwrap();assert!(command.contains("sudo -- python3 -u -- "));assert!(command.contains("'\"'\"'"));
        assert!(ProgramLaunch{path:"relative.py".into(),..Default::default()}.command().is_err());
        assert!(ProgramLaunch{path:"/tmp/a\n.py".into(),..Default::default()}.command().is_err());
        assert_eq!(ProgramLaunch{path:"/tmp/run.sh".into(),..Default::default()}.command().unwrap(),"cd -- '/tmp' && bash -- '/tmp/run.sh'");
    }
}
