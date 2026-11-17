//! Retire only the historical app's owned WebKit network-cache tree.
//!
//! `Library/WebKit/.../WebsiteData` is persistent storage (including LocalStorage
//! and IndexedDB), not disposable cache. It is deliberately outside this module.
use anyhow::{bail, ensure, Context, Result};
use std::{
    ffi::{CStr, CString, OsStr, OsString},
    fs::File,
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::{OsStrExt, OsStringExt},
    },
    path::{Component, Path, PathBuf},
};
const RELATIVE: &str = "Library/Caches/io.portcanvas.app/WebKit";
const CACHE_KINDS: &[&str] = &[
    "AlternativeServices",
    "NetworkCache",
    "HSTS",
    "CacheStorage",
];
const MAX_ENTRIES: usize = 200_000;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Identity {
    device: u64,
    inode: u64,
}
fn identity(s: &libc::stat) -> Identity {
    Identity {
        device: s.st_dev as u64,
        inode: s.st_ino as u64,
    }
}
#[derive(Debug)]
pub struct CleanupPlan {
    /// Read-only preview paths. Execution verifies these against the fixed scope.
    pub candidates: Vec<PathBuf>,
    home: PathBuf,
    expected: Option<Identity>,
}
#[derive(Debug, Default, serde::Serialize)]
pub struct CleanupReport {
    pub removed: Vec<PathBuf>,
    pub failures: Vec<String>,
}
fn name(s: &OsStr) -> Result<CString> {
    Ok(CString::new(s.as_bytes())?)
}
fn stat(fd: &File) -> Result<libc::stat> {
    let mut value = std::mem::MaybeUninit::uninit();
    ensure!(
        unsafe { libc::fstat(fd.as_raw_fd(), value.as_mut_ptr()) } == 0,
        "Cannot inspect cache directory: {}",
        io::Error::last_os_error()
    );
    Ok(unsafe { value.assume_init() })
}
fn child_stat(parent: &File, child: &OsStr) -> Result<libc::stat> {
    let mut value = std::mem::MaybeUninit::uninit();
    let child = name(child)?;
    ensure!(
        unsafe {
            libc::fstatat(
                parent.as_raw_fd(),
                child.as_ptr(),
                value.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0,
        "Cannot inspect legacy cache entry: {}",
        io::Error::last_os_error()
    );
    Ok(unsafe { value.assume_init() })
}
fn owned(s: &libc::stat) -> Result<()> {
    ensure!(
        s.st_uid == unsafe { libc::geteuid() },
        "Legacy cache entry is not owned by this account"
    );
    ensure!(
        s.st_mode & 0o022 == 0,
        "Legacy cache entry is writable by another account"
    );
    let kind = s.st_mode & libc::S_IFMT;
    ensure!(
        kind == libc::S_IFDIR || kind == libc::S_IFREG,
        "Linked or special legacy cache entry retained"
    );
    ensure!(
        kind != libc::S_IFREG || s.st_nlink == 1,
        "Hard-linked legacy cache entry retained"
    );
    Ok(())
}
fn open_child(parent: &File, child: &OsStr) -> io::Result<Option<File>> {
    let child =
        CString::new(child.as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            child.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd >= 0 {
        return Ok(Some(unsafe { File::from_raw_fd(fd) }));
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::NotFound {
        Ok(None)
    } else {
        Err(error)
    }
}
fn open_home(home: &Path) -> Result<File> {
    ensure!(
        home.is_absolute() && home != Path::new("/"),
        "Legacy cleanup requires an absolute account home"
    );
    let mut current = File::open("/")?;
    for part in home.components() {
        match part {
            Component::RootDir => (),
            Component::Normal(part) => {
                current = open_child(&current, part)?.context("Account home does not exist")?
            }
            _ => bail!("Legacy cleanup home contains a traversal component"),
        }
    }
    owned(&stat(&current)?)?;
    Ok(current)
}
fn cache_parent(home: &Path) -> Result<Option<File>> {
    let mut current = open_home(home)?;
    for part in ["Library", "Caches", "io.portcanvas.app"] {
        let Some(next) = open_child(&current, OsStr::new(part))? else {
            return Ok(None);
        };
        owned(&stat(&next)?)?;
        current = next;
    }
    Ok(Some(current))
}
struct Directory(*mut libc::DIR);
impl Drop for Directory {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0);
        }
    }
}
fn entries(fd: &File) -> Result<Vec<OsString>> {
    let copy = unsafe { libc::dup(fd.as_raw_fd()) };
    ensure!(
        copy >= 0,
        "Cannot enumerate legacy cache: {}",
        io::Error::last_os_error()
    );
    let raw = unsafe { libc::fdopendir(copy) };
    if raw.is_null() {
        unsafe {
            libc::close(copy);
        }
        bail!(
            "Cannot enumerate legacy cache: {}",
            io::Error::last_os_error()
        );
    }
    let dir = Directory(raw);
    // dup shares its directory offset; rewind before each validation/deletion pass.
    unsafe {
        libc::rewinddir(dir.0);
    }
    let mut out = vec![];
    loop {
        unsafe {
            *libc::__error() = 0;
        }
        let entry = unsafe { libc::readdir(dir.0) };
        if entry.is_null() {
            let errno = unsafe { *libc::__error() };
            ensure!(
                errno == 0,
                "Cannot read legacy cache: {}",
                io::Error::from_raw_os_error(errno)
            );
            break;
        }
        let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        ensure!(
            out.len() < MAX_ENTRIES,
            "Legacy cache contains too many entries; retained"
        );
        out.push(OsString::from_vec(bytes.to_vec()));
    }
    Ok(out)
}
fn persistent_name(value: &OsStr) -> bool {
    let lower = value.to_string_lossy().to_ascii_lowercase();
    lower.ends_with(".json")
        || matches!(
            lower.as_str(),
            "localstorage"
                | "indexeddb"
                | "websitedata"
                | "credentials"
                | "backgrounds"
                | "logs"
                | "migration-backups"
        )
}
fn validate_tree(fd: &File, root: bool, remaining: &mut usize, depth: usize) -> Result<()> {
    ensure!(depth <= 64, "Legacy cache is too deep; retained");
    owned(&stat(fd)?)?;
    for child in entries(fd)? {
        ensure!(
            *remaining > 0,
            "Legacy cache contains too many entries; retained"
        );
        *remaining -= 1;
        ensure!(
            !persistent_name(&child),
            "Possible user data inside legacy cache; retained"
        );
        let meta = child_stat(fd, &child)?;
        owned(&meta)?;
        if root {
            ensure!(
                CACHE_KINDS.iter().any(|v| child == OsStr::new(v))
                    && meta.st_mode & libc::S_IFMT == libc::S_IFDIR,
                "Mixed or unrecognized legacy WebKit directory; retained"
            );
        }
        if meta.st_mode & libc::S_IFMT == libc::S_IFDIR {
            let next =
                open_child(fd, &child)?.context("Cache directory changed during inspection")?;
            ensure!(
                identity(&meta) == identity(&stat(&next)?),
                "Cache directory changed during inspection"
            );
            validate_tree(&next, false, remaining, depth + 1)?;
        }
    }
    Ok(())
}
/// Preview without reading file contents or changing any cache/data.
/// Reject a mixed or unsafe tree in its entirety, before deleting any entry.
pub fn inspect(home: &Path) -> Result<CleanupPlan> {
    let mut plan = CleanupPlan {
        candidates: vec![],
        home: home.to_path_buf(),
        expected: None,
    };
    let Some(parent) = cache_parent(home)? else {
        return Ok(plan);
    };
    let Some(cache) = open_child(&parent, OsStr::new("WebKit"))? else {
        return Ok(plan);
    };
    validate_tree(&cache, true, &mut { MAX_ENTRIES }, 0)?;
    plan.expected = Some(identity(&stat(&cache)?));
    plan.candidates.push(home.join(RELATIVE));
    Ok(plan)
}
fn unlink(parent: &File, child: &OsStr, directory: bool) -> Result<()> {
    let child = name(child)?;
    ensure!(
        unsafe {
            libc::unlinkat(
                parent.as_raw_fd(),
                child.as_ptr(),
                if directory { libc::AT_REMOVEDIR } else { 0 },
            )
        } == 0,
        "Cannot remove retired legacy cache: {}",
        io::Error::last_os_error()
    );
    Ok(())
}
fn remove_contents(fd: &File, root: bool, remaining: &mut usize, depth: usize) -> Result<()> {
    ensure!(depth <= 64, "Legacy cache is too deep; retained");
    for child in entries(fd)? {
        ensure!(*remaining > 0, "Legacy cache changed while removing it");
        *remaining -= 1;
        ensure!(
            !persistent_name(&child),
            "Possible user data appeared in legacy cache; retained"
        );
        let meta = child_stat(fd, &child)?;
        owned(&meta)?;
        if root {
            ensure!(
                CACHE_KINDS.iter().any(|v| child == OsStr::new(v)),
                "Mixed cache content appeared; retained"
            );
        }
        if meta.st_mode & libc::S_IFMT == libc::S_IFDIR {
            let next =
                open_child(fd, &child)?.context("Cache directory changed while removing it")?;
            ensure!(
                identity(&meta) == identity(&stat(&next)?),
                "Cache directory changed while removing it"
            );
            remove_contents(&next, false, remaining, depth + 1)?;
            ensure!(
                identity(&meta) == identity(&child_stat(fd, &child)?),
                "Cache directory changed while removing it"
            );
            unlink(fd, &child, true)?;
        } else {
            ensure!(
                identity(&meta) == identity(&child_stat(fd, &child)?),
                "Cache file changed while removing it"
            );
            unlink(fd, &child, false)?;
        }
    }
    Ok(())
}
impl CleanupPlan {
    /// Call only after authenticated replacement and first-frame readiness (or an
    /// explicitly completed manual migration). A cleanup failure never rolls back code.
    pub fn execute_after_readiness(self) -> CleanupReport {
        let mut report = CleanupReport::default();
        let Some(expected) = self.expected else {
            return report;
        };
        let path = self.home.join(RELATIVE);
        let result = (|| -> Result<()> {
            ensure!(
                self.candidates == [path.clone()],
                "Cleanup preview was modified; retained"
            );
            let parent =
                cache_parent(&self.home)?.context("Legacy cache parent changed after preview")?;
            let cache = open_child(&parent, OsStr::new("WebKit"))?
                .context("Legacy cache changed after preview")?;
            ensure!(
                identity(&stat(&cache)?) == expected,
                "Legacy cache changed after preview; retained"
            );
            validate_tree(&cache, true, &mut { MAX_ENTRIES }, 0)?;
            remove_contents(&cache, true, &mut { MAX_ENTRIES }, 0)?;
            ensure!(
                identity(&child_stat(&parent, OsStr::new("WebKit"))?) == expected,
                "Legacy cache changed while removing it"
            );
            unlink(&parent, OsStr::new("WebKit"), true)
        })();
        match result {
            Ok(()) => report.removed.push(path),
            Err(error) => report.failures.push(format!(
                "Legacy cache retained at {}: {error:#}",
                path.display()
            )),
        }
        report
    }
}
/// Updater convenience entry. Isolated tests and custom data directories never
/// clean the real account's historical cache as a side effect.
pub fn current_user_after_readiness() -> CleanupReport {
    if std::env::var_os("WCS_DATA_DIR").is_some() || std::env::var_os("WCS_TEST_ROOT").is_some() {
        return CleanupReport::default();
    }
    let result = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("Account HOME is unavailable")
        .and_then(|home| inspect(&home));
    match result {
        Ok(plan) => plan.execute_after_readiness(),
        Err(error) => CleanupReport {
            failures: vec![format!("Legacy cache retained: {error:#}")],
            ..Default::default()
        },
    }
}
