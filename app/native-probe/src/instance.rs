//! One native writer per data directory; stable and preview directories remain independent.
use anyhow::{Context, Result};
use std::{fs::{File, OpenOptions}, os::windows::fs::OpenOptionsExt, path::Path};
pub fn acquire(data:&Path)->Result<File>{
    OpenOptions::new().read(true).write(true).create(true).truncate(false).share_mode(0)
        .open(data.join(".native-instance"))
        .context("无法独占预览版资料目录，其他预览版可能仍在运行。")
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn data_directory_lease_rejects_a_second_writer_and_releases_on_drop(){
        let root=std::path::PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).unwrap();let first=acquire(&root).unwrap();assert!(acquire(&root).is_err());drop(first);
        drop(acquire(&root).unwrap());std::fs::remove_file(root.join(".native-instance")).unwrap();std::fs::remove_dir(root).unwrap();
    }
}
