use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Refuse to read a keybindings/template source past this size. Real files are at most a few
/// hundred KB even with thousands of binds; this is just a safety net against something else
/// (a malicious `.hbt` template someone hands you, a symlink into an unexpectedly huge file)
/// getting read fully into memory.
const MAX_SOURCE_FILE_BYTES: u64 = 16 * 1024 * 1024;

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

/// Like `fs::read_to_string`, but refuses to read a file larger than `MAX_SOURCE_FILE_BYTES`
/// rather than pulling it fully into memory first.
pub fn read_to_string_capped(path: &Path) -> io::Result<String> {
    let size = fs::metadata(path)?.len();
    if size > MAX_SOURCE_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "file is {size} bytes, over the {MAX_SOURCE_FILE_BYTES}-byte limit hyprbind reads"
            ),
        ));
    }
    fs::read_to_string(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_to_string_capped_reads_a_normal_sized_file() {
        let path = std::env::temp_dir().join(format!(
            "hyprbind-test-read-capped-small-{}.conf",
            std::process::id()
        ));
        fs::write(&path, "bind = SUPER, Q, killactive\n").unwrap();

        assert_eq!(
            read_to_string_capped(&path).unwrap(),
            "bind = SUPER, Q, killactive\n"
        );

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn read_to_string_capped_refuses_a_file_over_the_limit() {
        let path = std::env::temp_dir().join(format!(
            "hyprbind-test-read-capped-huge-{}.conf",
            std::process::id()
        ));
        // A sparse file: claims to be over the limit without actually writing that much data.
        let file = fs::File::create(&path).unwrap();
        file.set_len(MAX_SOURCE_FILE_BYTES + 1).unwrap();

        let err = read_to_string_capped(&path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);

        fs::remove_file(&path).unwrap();
    }
}
