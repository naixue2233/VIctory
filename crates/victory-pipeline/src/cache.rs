use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::Converter;

/// Identifies one conversion result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CacheKey(blake3::Hash);

impl CacheKey {
    pub fn new(converter: &dyn Converter, input: &[u8]) -> Self {
        let mut h = blake3::Hasher::new();
        h.update(converter.id().as_bytes());
        h.update(&[0]);
        h.update(&converter.version().to_le_bytes());
        h.update(input);
        CacheKey(h.finalize())
    }

    pub fn to_hex(&self) -> String {
        self.0.to_hex().to_string()
    }
}

/// Content-addressed store under the user's cache directory.
#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    /// Opens (creating) a cache at `root`. Refuses locations inside the repo.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        victory_core::ensure_outside_repo(&root)?;
        std::fs::create_dir_all(&root).with_context(|| format!("creating {}", root.display()))?;
        Ok(Cache { root })
    }

    pub fn path_for(&self, key: &CacheKey) -> PathBuf {
        let hex = key.to_hex();
        self.root.join(&hex[..2]).join(&hex[2..])
    }

    /// Returns the cached output for `input`, converting it first if needed.
    pub fn get_or_convert(&self, converter: &dyn Converter, input: &[u8]) -> Result<PathBuf> {
        let key = CacheKey::new(converter, input);
        let path = self.path_for(&key);
        if path.is_file() {
            tracing::trace!(converter = converter.id(), key = %key.to_hex(), "cache hit");
            return Ok(path);
        }
        let output = converter
            .convert(input)
            .with_context(|| format!("converter {} v{}", converter.id(), converter.version()))?;
        let dir = path.parent().expect("hashed path has a parent");
        std::fs::create_dir_all(dir)?;
        // Write-then-rename so readers never see partial output.
        let tmp = tempfile_in(dir);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        let written = file.write_all(&output);
        drop(file);
        let published = written.and_then(|()| std::fs::rename(&tmp, &path));
        if let Err(e) = published {
            let _ = std::fs::remove_file(&tmp);
            // Another caller published the same key first. Entries are
            // content-addressed, so its bytes are ours. On Windows the rename
            // over it fails (os error 5) while that entry is open, for example
            // by the worker reading it back or by a scanner.
            if path.is_file() {
                tracing::trace!(converter = converter.id(), key = %key.to_hex(), error = %e, "entry published by another caller");
                return Ok(path);
            }
            return Err(e.into());
        }
        Ok(path)
    }
}

fn tempfile_in(dir: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(".tmp-{}-{n}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Upper {
        version: u32,
        runs: AtomicUsize,
    }

    impl Converter for Upper {
        fn id(&self) -> &'static str {
            "test.upper"
        }
        fn version(&self) -> u32 {
            self.version
        }
        fn convert(&self, input: &[u8]) -> Result<Vec<u8>> {
            self.runs.fetch_add(1, Ordering::Relaxed);
            Ok(input.to_ascii_uppercase())
        }
    }

    #[test]
    fn converts_once_then_hits() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = Cache::open(tmp.path().join("cache")).unwrap();
        let conv = Upper {
            version: 1,
            runs: AtomicUsize::new(0),
        };
        let a = cache.get_or_convert(&conv, b"abc").unwrap();
        let b = cache.get_or_convert(&conv, b"abc").unwrap();
        assert_eq!(a, b);
        assert_eq!(std::fs::read(&a).unwrap(), b"ABC");
        assert_eq!(conv.runs.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn failed_publication_removes_the_staging_file() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = Cache::open(tmp.path().join("cache")).unwrap();
        let conv = Upper {
            version: 1,
            runs: AtomicUsize::new(0),
        };
        let path = cache.path_for(&CacheKey::new(&conv, b"abc"));
        std::fs::create_dir_all(&path).unwrap();

        assert!(cache.get_or_convert(&conv, b"abc").is_err());
        assert!(path.is_dir());
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn concurrent_callers_all_get_the_entry() {
        // The title's pipeline workers translate the same shader at once and
        // read the entry back straight away: publishing must not fail for the
        // ones that lose the race to another worker.
        let tmp = tempfile::tempdir().unwrap();
        let cache = Cache::open(tmp.path().join("cache")).unwrap();
        let conv = Upper {
            version: 1,
            runs: AtomicUsize::new(0),
        };
        let threads = 16;
        let barrier = std::sync::Barrier::new(threads);
        std::thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| {
                    for round in 0..50u32 {
                        let input = format!("abc{round}");
                        barrier.wait();
                        let path = cache.get_or_convert(&conv, input.as_bytes()).unwrap();
                        assert_eq!(
                            std::fs::read(&path).unwrap(),
                            input.to_ascii_uppercase().as_bytes()
                        );
                    }
                });
            }
        });
        for shard in std::fs::read_dir(tmp.path().join("cache")).unwrap() {
            for entry in std::fs::read_dir(shard.unwrap().path()).unwrap() {
                let name = entry.unwrap().file_name();
                assert!(
                    !name.to_string_lossy().starts_with(".tmp-"),
                    "staging file left: {name:?}"
                );
            }
        }
    }

    /// Publishes its own entry while converting, as another worker that won
    /// the race would, and keeps it open the way a reader or a scanner does.
    struct Racer {
        path: std::sync::OnceLock<PathBuf>,
        held: std::sync::Mutex<Option<std::fs::File>>,
    }

    impl Converter for Racer {
        fn id(&self) -> &'static str {
            "test.racer"
        }
        fn version(&self) -> u32 {
            1
        }
        fn convert(&self, input: &[u8]) -> Result<Vec<u8>> {
            let output = input.to_ascii_uppercase();
            let path = self.path.get().unwrap();
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, &output)?;
            let mut open = std::fs::OpenOptions::new();
            open.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                // FILE_SHARE_READ only: replacing the file is refused.
                open.share_mode(1);
            }
            *self.held.lock().unwrap() = Some(open.open(path)?);
            Ok(output)
        }
    }

    #[test]
    fn losing_the_publication_race_uses_the_winners_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = Cache::open(tmp.path().join("cache")).unwrap();
        let conv = Racer {
            path: std::sync::OnceLock::new(),
            held: std::sync::Mutex::new(None),
        };
        let path = cache.path_for(&CacheKey::new(&conv, b"abc"));
        conv.path.set(path.clone()).unwrap();

        assert_eq!(cache.get_or_convert(&conv, b"abc").unwrap(), path);
        assert_eq!(std::fs::read(&path).unwrap(), b"ABC");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1,
            "the staging file is removed"
        );
    }

    #[test]
    fn version_bump_invalidates() {
        let v1 = Upper {
            version: 1,
            runs: AtomicUsize::new(0),
        };
        let v2 = Upper {
            version: 2,
            runs: AtomicUsize::new(0),
        };
        assert_ne!(CacheKey::new(&v1, b"x"), CacheKey::new(&v2, b"x"));
    }

    #[test]
    fn refuses_cache_inside_repo() {
        let inside = Path::new(env!("CARGO_MANIFEST_DIR")).join("cache");
        assert!(Cache::open(inside).is_err());
    }
}
