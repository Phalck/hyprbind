use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::fs_util::write_atomic;

/// Replace line `line_no` (1-based) of `contents` with `new_line`, preserving every other line
/// and whether the file ended with a trailing newline. Returns `None` if `line_no` is out of
/// range for `contents`, or if `expected_old_line` is given and doesn't match what's actually
/// there — either way, the file changed on disk since it was parsed.
fn replace_line(
    contents: &str,
    line_no: usize,
    expected_old_line: Option<&str>,
    new_line: &str,
) -> Option<String> {
    let mut lines: Vec<&str> = contents.lines().collect();
    let idx = line_no.checked_sub(1)?;
    if idx >= lines.len() {
        return None;
    }
    if let Some(expected) = expected_old_line
        && lines[idx] != expected
    {
        return None;
    }
    lines[idx] = new_line;

    let mut result = lines.join("\n");
    if contents.ends_with('\n') {
        result.push('\n');
    }
    Some(result)
}

/// Read `path`, replace line `line_no` with `new_line`, and write the result back atomically.
/// `expected_old_line`, when given, must match the line's current on-disk text or the write is
/// refused — guards against clobbering a change made by another hyprbind instance or a hand edit
/// since this line was last loaded (the file can change without its line count changing, which a
/// bounds check alone wouldn't catch).
pub(super) fn write_line(
    path: &Path,
    line_no: usize,
    expected_old_line: Option<&str>,
    new_line: &str,
) -> io::Result<()> {
    let contents = fs::read_to_string(path)?;
    let updated =
        replace_line(&contents, line_no, expected_old_line, new_line).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "source file changed on disk; reload and try again",
            )
        })?;
    write_atomic(path, &updated)
}

/// Remove line `line_no` (1-based) from `contents` entirely, preserving every other line.
/// Returns `None` if `line_no` is out of range, or if `expected_old_line` is given and doesn't
/// match what's actually there (the file changed on disk since it was parsed).
fn remove_line(contents: &str, line_no: usize, expected_old_line: Option<&str>) -> Option<String> {
    let mut lines: Vec<&str> = contents.lines().collect();
    let idx = line_no.checked_sub(1)?;
    if idx >= lines.len() {
        return None;
    }
    if let Some(expected) = expected_old_line
        && lines[idx] != expected
    {
        return None;
    }
    lines.remove(idx);

    let mut result = lines.join("\n");
    if contents.ends_with('\n') && !lines.is_empty() {
        result.push('\n');
    }
    Some(result)
}

/// Read `path`, remove line `line_no`, and write the result back atomically. `expected_old_line`
/// is the same optimistic-concurrency guard as `write_line`'s.
pub(super) fn delete_line(
    path: &Path,
    line_no: usize,
    expected_old_line: Option<&str>,
) -> io::Result<()> {
    let contents = fs::read_to_string(path)?;
    let updated = remove_line(&contents, line_no, expected_old_line).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "source file changed on disk; reload and try again",
        )
    })?;
    write_atomic(path, &updated)
}

/// Append `new_lines` to the end of `path`, preceded by a blank line and `marker` (a full `#
/// ...` comment line), and write the result back atomically. Every existing line is left
/// untouched.
pub(super) fn append_lines(path: &Path, marker: &str, new_lines: &[String]) -> io::Result<()> {
    let mut contents = fs::read_to_string(path)?;
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push('\n');
    contents.push_str(marker);
    contents.push('\n');
    for line in new_lines {
        contents.push_str(line);
        contents.push('\n');
    }
    write_atomic(path, &contents)
}

/// Write `lines` (already newline-terminated content, one shortcut per line) to `path`,
/// creating `folder` first if it doesn't exist yet.
pub(super) fn write_template(folder: &Path, path: &Path, lines: &[String]) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    let mut contents = String::new();
    for line in lines {
        contents.push_str(line);
        contents.push('\n');
    }
    write_atomic(path, &contents)
}

/// List files directly inside `folder` whose extension matches `extension`, sorted by name.
/// Returns an empty list if the folder doesn't exist or can't be read, rather than failing.
pub(super) fn list_files_with_extension(folder: &Path, extension: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some(extension))
        .collect();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::super::backup::BACKUP_EXTENSION;
    use super::super::template::TEMPLATE_EXTENSION;
    use super::super::test_support::sample_shortcut;
    use super::*;

    #[test]
    fn replace_line_swaps_only_target_line() {
        let contents = "one\ntwo\nthree\n";
        let updated = replace_line(contents, 2, None, "TWO").unwrap();
        assert_eq!(updated, "one\nTWO\nthree\n");
    }

    #[test]
    fn replace_line_preserves_missing_trailing_newline() {
        let contents = "one\ntwo\nthree";
        let updated = replace_line(contents, 1, None, "ONE").unwrap();
        assert_eq!(updated, "ONE\ntwo\nthree");
    }

    #[test]
    fn replace_line_out_of_range_returns_none() {
        let contents = "one\ntwo\n";
        assert!(replace_line(contents, 5, None, "x").is_none());
        assert!(replace_line(contents, 0, None, "x").is_none());
    }

    #[test]
    fn replace_line_matching_expected_old_line_succeeds() {
        let contents = "one\ntwo\nthree\n";
        let updated = replace_line(contents, 2, Some("two"), "TWO").unwrap();
        assert_eq!(updated, "one\nTWO\nthree\n");
    }

    #[test]
    fn replace_line_refuses_when_expected_old_line_does_not_match() {
        let contents = "one\ntwo\nthree\n";
        assert!(
            replace_line(contents, 2, Some("something else"), "TWO").is_none(),
            "the file changed since expected_old_line was captured, so the write must be refused"
        );
    }

    #[test]
    fn remove_line_drops_only_the_target_line() {
        let contents = "one\ntwo\nthree\n";
        let updated = remove_line(contents, 2, None).unwrap();
        assert_eq!(updated, "one\nthree\n");
    }

    #[test]
    fn remove_line_preserves_missing_trailing_newline() {
        let contents = "one\ntwo\nthree";
        let updated = remove_line(contents, 3, None).unwrap();
        assert_eq!(updated, "one\ntwo");
    }

    #[test]
    fn remove_line_of_the_only_line_leaves_an_empty_file() {
        let contents = "only\n";
        let updated = remove_line(contents, 1, None).unwrap();
        assert_eq!(updated, "");
    }

    #[test]
    fn remove_line_out_of_range_returns_none() {
        let contents = "one\ntwo\n";
        assert!(remove_line(contents, 5, None).is_none());
        assert!(remove_line(contents, 0, None).is_none());
    }

    #[test]
    fn remove_line_refuses_when_expected_old_line_does_not_match() {
        let contents = "one\ntwo\nthree\n";
        assert!(
            remove_line(contents, 2, Some("something else")).is_none(),
            "the file changed since expected_old_line was captured, so the delete must be refused"
        );
    }

    #[test]
    fn write_template_creates_folder_and_writes_resolved_lines() {
        let dir = std::env::temp_dir().join(format!("hyprbind-test-{}", std::process::id()));
        let folder = dir.join("templates");
        let path = folder.join("test.hbt");

        let lines = vec![sample_shortcut(1, "Q").resolved_line()];
        write_template(&folder, &path, &lines).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "bind = SUPER, Q, exec, foo\n");
        assert!(!fs::exists(path.with_extension("hbt.tmp")).unwrap_or(false));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn append_lines_adds_marker_comment_and_preserves_existing_content() {
        let path =
            std::env::temp_dir().join(format!("hyprbind-test-append-{}.conf", std::process::id()));
        fs::write(&path, "bind = $mainMod, Q, killactive\n").unwrap();

        append_lines(
            &path,
            "# Applied from template: gaming.hbt",
            &["bind = SUPER, W, exec, foo".to_string()],
        )
        .unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert_eq!(
            contents,
            "bind = $mainMod, Q, killactive\n\n# Applied from template: gaming.hbt\nbind = SUPER, W, exec, foo\n"
        );

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn list_files_with_extension_filters_and_sorts() {
        let dir = std::env::temp_dir().join(format!("hyprbind-test-list-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("b.hbt"), "").unwrap();
        fs::write(dir.join("a.hbt"), "").unwrap();
        fs::write(dir.join("notes.txt"), "").unwrap();
        fs::write(dir.join("c.hbb"), "").unwrap();

        assert_eq!(
            list_files_with_extension(&dir, TEMPLATE_EXTENSION),
            vec![dir.join("a.hbt"), dir.join("b.hbt")]
        );
        assert_eq!(
            list_files_with_extension(&dir, BACKUP_EXTENSION),
            vec![dir.join("c.hbb")]
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn list_files_with_extension_on_missing_folder_returns_empty() {
        let missing = std::env::temp_dir().join("hyprbind-does-not-exist-hopefully");
        assert!(list_files_with_extension(&missing, TEMPLATE_EXTENSION).is_empty());
    }
}
