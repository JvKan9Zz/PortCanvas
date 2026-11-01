//! Native file selections and request ownership. Paths/names remain untranslated.
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeSet;
#[derive(Clone)]
pub struct Entry {
    pub name: String,
    pub kind: String,
    pub size: Option<u64>,
    pub modified: Option<u64>,
}
#[derive(Default)]
pub struct Panel {
    pub path: String,
    pub parent: String,
    pub home: String,
    pub entries: Vec<Entry>,
    pub selected: BTreeSet<String>,
    pub loaded: bool,
    pub loading: bool,
    pub error: String,
    pub generation: u64,
    serial: u64,
    anchor: Option<usize>,
}
impl Panel {
    pub fn begin(&mut self) -> u64 {
        self.serial += 1;
        self.loading = true;
        self.error.clear();
        self.serial
    }
    pub fn reset(&mut self) {
        let serial = self.serial + 1;
        *self = Self::default();
        self.serial = serial;
    }
    pub fn receive(&mut self, ticket: u64, result: Result<Value>) -> bool {
        if self.serial != ticket {
            return false;
        }
        self.loading = false;
        match result.and_then(|data| {
            let entries = data["entries"]
                .as_array()
                .context("文件列表格式无效。")?
                .iter()
                .map(|e| {
                    Ok(Entry {
                        name: e["name"].as_str().context("文件名无效。")?.into(),
                        kind: e["type"].as_str().context("文件类型无效。")?.into(),
                        size: e["size"].as_u64(),
                        modified: e["modified"].as_u64(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((data, entries))
        }) {
            Ok((data, entries)) => {
                self.path = data["path"].as_str().unwrap_or_default().into();
                self.parent = data["parent"].as_str().unwrap_or_default().into();
                self.home = data["home"].as_str().unwrap_or_default().into();
                self.generation = data["generation"].as_u64().unwrap_or_default();
                self.entries = entries;
                self.selected.clear();
                self.anchor = None;
                self.loaded = true;
                self.error.clear();
            }
            Err(e) => self.error = e.to_string(),
        }
        true
    }
    pub fn choose(&mut self, index: usize, add: bool, range: bool) {
        if index >= self.entries.len() {
            return;
        }
        if range {
            let from = self.anchor.unwrap_or(index);
            if !add {
                self.selected.clear();
            }
            for n in from.min(index)..=from.max(index) {
                self.selected.insert(self.entries[n].name.clone());
            }
        } else {
            let name = &self.entries[index].name;
            if !add {
                self.selected.clear();
            }
            if !self.selected.insert(name.clone()) {
                self.selected.remove(name);
            }
            self.anchor = Some(index);
        }
    }
    pub fn all(&mut self, checked: bool) {
        self.selected = if checked {
            self.entries.iter().map(|e| e.name.clone()).collect()
        } else {
            BTreeSet::new()
        };
    }
    pub fn folder(&self, index: usize, remote: bool) -> Option<String> {
        let e = self.entries.get(index)?;
        if e.kind != "directory" {
            return None;
        }
        Some(if remote {
            format!("{}/{}", self.path.trim_end_matches('/'), e.name)
        } else {
            std::path::Path::new(&self.path)
                .join(&e.name)
                .to_string_lossy()
                .into_owned()
        })
    }
}
pub fn bytes(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1048576 {
        format!("{:.1} KiB", n as f64 / 1024.)
    } else if n < 1073741824 {
        format!("{:.1} MiB", n as f64 / 1048576.)
    } else {
        format!("{:.2} GiB", n as f64 / 1073741824.)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn data() -> Value {
        json!({"generation":2,"path":"/中文","parent":"/","home":"/home","entries":[{"name":"设置","type":"directory"},{"name":"a","type":"file","size":5},{"name":"b","type":"file"}]})
    }
    #[test]
    fn late_reads_and_replaced_connections_cannot_repopulate_selection() {
        let mut p = Panel::default();
        let old = p.begin();
        let new = p.begin();
        assert!(!p.receive(old, Ok(data())));
        assert!(p.receive(new, Ok(data())));
        p.choose(0, false, false);
        assert!(p.selected.contains("设置"));
        let old = p.begin();
        p.reset();
        assert!(!p.receive(old, Ok(data())));
        assert!(!p.loaded && p.selected.is_empty());
    }
    #[test]
    fn multi_selection_ranges_folders_and_failures_preserve_names() {
        let mut p = Panel::default();
        let id = p.begin();
        p.receive(id, Ok(data()));
        p.choose(0, false, false);
        p.choose(2, false, true);
        assert_eq!(p.selected.len(), 3);
        p.choose(1, true, false);
        assert_eq!(p.selected.len(), 2);
        assert_eq!(p.folder(0, true).unwrap(), "/中文/设置");
        assert!(p.folder(1, true).is_none());
        let id = p.begin();
        p.receive(id, Err(anyhow::anyhow!("missing")));
        assert_eq!(p.path, "/中文");
        assert_eq!(p.entries.len(), 3);
        assert_eq!(p.error, "missing");
        p.all(false);
        assert!(p.selected.is_empty());
    }
}
