use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Atomically write `contents` to `path` (write to a sibling `.tmp` file, then rename over the
/// original) so a failed write can never leave a partially-written file behind.
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let tmp_path = {
        let mut s = path.as_os_str().to_owned();
        s.push(".tmp");
        PathBuf::from(s)
    };
    fs::write(&tmp_path, contents)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}
