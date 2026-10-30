//! Native log-view state; storage, retention and deletion remain in the shared service.
use portcanvas_core::log_store::{Cursor, Query};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub records: Vec<Value>,
    pub total: u64,
    pub has_more: bool,
    pub cursor: Option<Cursor>,
    pub projects: Vec<(String, String)>,
    pub days: Vec<String>,
    pub notice: String,
    pub usage_bytes: u64,
}
pub fn removable(record: &Value) -> bool {
    record["active"] != true && record["status"] != "running"
}
pub struct Browser {
    pub root: PathBuf,
    pub query: Query,
    pub page: Page,
    pub history: Vec<Option<Cursor>>,
    pub checked: BTreeSet<String>,
    pub selected: Option<String>,
    pub detail: Option<Value>,
    pub loading: bool,
    pub reload: bool,
    pub reading: bool,
    pub reread: bool,
    pub version: u64,
    pub detail_version: u64,
}
impl Browser {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            query: Query::default(),
            page: Page::default(),
            history: Vec::new(),
            checked: BTreeSet::new(),
            selected: None,
            detail: None,
            loading: false,
            reload: false,
            reading: false,
            reread: false,
            version: 0,
            detail_version: 0,
        }
    }
    pub fn sync_root(&mut self, root: PathBuf) {
        if self.root == root {
            return;
        }
        let (loading, reading, version, detail_version) = (
            self.loading,
            self.reading,
            self.version + 1,
            self.detail_version + 1,
        );
        *self = Self::new(root);
        self.loading = loading;
        self.reading = reading;
        self.version = version;
        self.detail_version = detail_version;
        self.reload = loading;
    }
    pub fn filters(&mut self, project: String, day: String, status: String) -> bool {
        if self.query.project == project && self.query.day == day && self.query.status == status {
            return false;
        }
        self.query = Query {
            project,
            day,
            status,
            cursor: None,
        };
        self.history.clear();
        self.checked.clear();
        self.version += 1;
        true
    }
    pub fn project_options(&self) -> Vec<(String, String)> {
        let mut options = self.page.projects.clone();
        if !self.query.project.is_empty() && !options.iter().any(|p| p.0 == self.query.project) {
            options.push((self.query.project.clone(), self.query.project.clone()));
        }
        options
    }
    pub fn day_options(&self) -> Vec<String> {
        let mut options = self.page.days.clone();
        if !self.query.day.is_empty() && !options.contains(&self.query.day) {
            options.push(self.query.day.clone());
        }
        options
    }
    pub fn next(&mut self) -> bool {
        if self.loading || !self.page.has_more {
            return false;
        }
        self.history.push(self.query.cursor.clone());
        self.query.cursor = self.page.cursor.clone();
        self.checked.clear();
        self.version += 1;
        true
    }
    pub fn back(&mut self) -> bool {
        if self.loading {
            return false;
        }
        let Some(cursor) = self.history.pop() else {
            return false;
        };
        self.query.cursor = cursor;
        self.checked.clear();
        self.version += 1;
        true
    }
    pub fn begin_page(&mut self) -> Option<(u64, Query)> {
        if self.loading {
            self.reload = true;
            return None;
        }
        self.loading = true;
        self.reload = false;
        Some((self.version, self.query.clone()))
    }
    pub fn finish_page(&mut self, version: u64, page: Option<Page>) -> bool {
        self.loading = false;
        if version != self.version {
            self.reload = true;
            return false;
        }
        if let Some(page) = page {
            self.checked
                .retain(|id| page.records.iter().any(|r| r["id"] == *id && removable(r)));
            self.page = page;
        }
        true
    }
    pub fn toggle(&mut self, id: &str, checked: bool) {
        if checked
            && self
                .page
                .records
                .iter()
                .any(|r| r["id"] == id && removable(r))
        {
            self.checked.insert(id.into());
        } else {
            self.checked.remove(id);
        }
    }
    pub fn select_all(&mut self, checked: bool) {
        self.checked = if checked {
            self.page
                .records
                .iter()
                .filter(|r| removable(r))
                .filter_map(|r| r["id"].as_str().map(str::to_owned))
                .collect()
        } else {
            BTreeSet::new()
        };
    }
    pub fn select(&mut self, id: String) {
        if self.selected.as_ref() != Some(&id) {
            self.selected = Some(id);
            self.detail = None;
            self.detail_version += 1;
        }
    }
    pub fn begin_detail(&mut self) -> Option<(u64, String)> {
        let id = self.selected.clone()?;
        if self.reading {
            self.reread = true;
            return None;
        }
        self.reading = true;
        self.reread = false;
        Some((self.detail_version, id))
    }
    pub fn finish_detail(&mut self, version: u64, detail: Option<Value>) -> bool {
        self.reading = false;
        if version != self.detail_version {
            self.reread = self.selected.is_some();
            return false;
        }
        self.detail = detail;
        true
    }
    pub fn clear_detail(&mut self) {
        self.selected = None;
        self.detail = None;
        self.detail_version += 1;
    }
    pub fn selected_running(&self) -> bool {
        self.detail
            .as_ref()
            .is_some_and(|r| r["active"] == true || r["status"] == "running")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portcanvas_core::{log_protocol::Marker, log_store::LogStore};
    fn fixture() -> (PathBuf, LogStore) {
        let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").expect("isolated test root"))
            .join(format!("native-logs-{}", uuid::Uuid::new_v4()));
        (root.clone(), LogStore::new(root).unwrap())
    }
    fn marker(n: usize) -> Marker {
        Marker {
            kind: "begin".into(),
            shell: "native-fixture".into(),
            sequence: n.to_string(),
            code: Some(0),
            command: format!("合成日志 {n}"),
            cwd: "/fixture/设置".into(),
            project_root: String::new(),
            end_cwd: String::new(),
            background_jobs: Vec::new(),
        }
    }
    fn load(browser: &mut Browser, store: &mut LogStore) {
        let (version, query) = browser.begin_page().unwrap();
        assert!(browser.finish_page(
            version,
            Some(serde_json::from_value(store.list(query).unwrap()).unwrap())
        ));
    }
    #[test]
    fn real_store_pages_filter_without_dropping_or_translating_user_data() {
        let (root, mut store) = fixture();
        for n in 0..207 {
            let mut marker = marker(n);
            if n == 3 {
                marker.code = Some(7);
            }
            let id = store
                .begin(&marker, "fixture", "", false, "command")
                .unwrap()
                .unwrap();
            store.append(&id, "中文原文\n").unwrap();
            store.finish(&id, Some(&marker), None).unwrap();
        }
        let mut browser = Browser::new(root.clone());
        load(&mut browser, &mut store);
        assert_eq!(browser.page.records.len(), 200);
        assert_eq!(browser.page.total, 207);
        assert_eq!(browser.page.projects[0].1, "设置");
        let first = browser
            .page
            .records
            .iter()
            .map(|r| r["id"].as_str().unwrap().to_owned())
            .collect::<BTreeSet<_>>();
        browser.select_all(true);
        assert_eq!(browser.checked.len(), 200);
        assert!(browser.next());
        assert!(browser.checked.is_empty());
        load(&mut browser, &mut store);
        assert_eq!(browser.page.records.len(), 7);
        assert!(!browser.page.has_more);
        assert!(browser
            .page
            .records
            .iter()
            .all(|r| !first.contains(r["id"].as_str().unwrap())));
        assert!(browser.back());
        load(&mut browser, &mut store);
        assert_eq!(browser.page.records[0]["project"]["name"], "设置");
        assert!(browser.filters(String::new(), String::new(), "failed".into()));
        load(&mut browser, &mut store);
        assert_eq!(browser.page.total, 1);
        assert_eq!(browser.page.records[0]["command"], "合成日志 3");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn changed_filter_and_root_reject_in_flight_results_and_coalesce_refreshes() {
        let (root, mut store) = fixture();
        let id = store
            .begin(&marker(0), "fixture", "", false, "command")
            .unwrap()
            .unwrap();
        store.finish(&id, Some(&marker(0)), None).unwrap();
        let mut browser = Browser::new(root.clone());
        let (old, query) = browser.begin_page().unwrap();
        assert!(browser.begin_page().is_none());
        browser.filters(String::new(), String::new(), "failed".into());
        assert!(!browser.finish_page(
            old,
            Some(serde_json::from_value(store.list(query).unwrap()).unwrap())
        ));
        assert!(browser.page.records.is_empty());
        assert!(browser.reload);
        load(&mut browser, &mut store);
        assert_eq!(browser.page.total, 0);
        browser.select(id.clone());
        let (ticket, _) = browser.begin_detail().unwrap();
        browser.sync_root(root.join("other"));
        assert!(!browser.finish_detail(ticket, Some(store.detail(&id).unwrap())));
        assert!(browser.detail.is_none());
        assert!(browser.selected.is_none());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn live_records_cannot_enter_native_cleanup_selection() {
        let (root, mut store) = fixture();
        let active = store
            .begin(&marker(0), "fixture", "", false, "command")
            .unwrap()
            .unwrap();
        let done = store
            .begin(&marker(1), "fixture", "", false, "command")
            .unwrap()
            .unwrap();
        store.finish(&done, Some(&marker(1)), None).unwrap();
        let mut browser = Browser::new(root.clone());
        load(&mut browser, &mut store);
        browser.toggle(&active, true);
        assert!(browser.checked.is_empty());
        browser.select_all(true);
        assert_eq!(browser.checked, BTreeSet::from([done]));
        browser.select(active.clone());
        let (ticket, _) = browser.begin_detail().unwrap();
        browser.finish_detail(ticket, Some(store.detail(&active).unwrap()));
        assert!(browser.selected_running());
        store.close("fixture end").unwrap();
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
