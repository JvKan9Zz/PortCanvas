use crate::preferences::Language;
use std::{collections::BTreeMap,sync::OnceLock};
static WORDS:OnceLock<BTreeMap<String,String>>=OnceLock::new();
pub fn text(input:&str,language:Language)->String {
    if language!=Language::English{return input.into();}
    let words=WORDS.get_or_init(||serde_json::from_str(include_str!("../../frontend/en.json")).expect("built-in language catalog"));
    if let Some(value)=words.get(input){return value.clone();}
    let token=regex::Regex::new(r"\{(\w+)\}").unwrap();
    let mut patterns=words.iter().filter(|(key,_)|key.contains('{')).collect::<Vec<_>>();
    patterns.sort_by_key(|(key,_)|std::cmp::Reverse(token.replace_all(key,"").len()));
    for(key,value)in patterns{
        let mut pattern=String::from("(?s)^");let mut end=0;let mut names=Vec::new();
        for found in token.captures_iter(key){let item=found.get(0).unwrap();pattern.push_str(&regex::escape(&key[end..item.start()]));pattern.push_str("(.*?)");names.push(found[1].to_string());end=item.end();}
        pattern.push_str(&regex::escape(&key[end..]));pattern.push('$');
        if let Some(captures)=regex::Regex::new(&pattern).unwrap().captures(input){let mut result=value.clone();for(index,name)in names.iter().enumerate(){result=result.replace(&format!("{{{name}}}"),&captures[index+1]);}return result;}
    }input.into()
}
#[cfg(test)]mod tests {
    use super::*;
    #[test]fn specific_labels_win_and_user_data_is_preserved(){
        assert_eq!(text("终端 2远端输入",Language::English),"Terminal 2 remote input");
        assert_eq!(text("确认覆盖文件",Language::Chinese),"确认覆盖文件");
        let message="目标已有 1 个同名文件，是否覆盖？\n/tmp/设置\n取消将保留原文件。";
        let translated=text(message,Language::English);assert!(translated.starts_with("Overwrite 1 existing files?"));assert!(translated.contains("/tmp/设置"));assert!(!translated.contains("/tmp/Settings"));
    }
}
