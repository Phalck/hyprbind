use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::parse_file;

/// Search `root` (recursively) for `.conf` or `.lua` files containing Hyprland `bind` lines
/// (either syntax), and return whichever has the most, as a best guess for "the" keybindings
/// file.
///
/// This makes no assumption about which dotfiles framework (if any) is in use: it just looks
/// for the file with the most recognizable shortcuts under the one location every Hyprland
/// install has, regardless of distribution.
pub fn discover(root: &Path) -> Option<PathBuf> {
    let mut visited = HashSet::new();
    keybinding_files(root, 6, &mut visited)
        .into_iter()
        .filter_map(|path| {
            let count = parse_file(&path).ok()?.shortcuts.len();
            (count > 0).then_some((path, count))
        })
        .max_by_key(|(_, count)| *count)
        .map(|(path, _)| path)
}

/// Collect every `.conf` or `.lua` file under `dir`, following symlinked directories (several
/// dotfiles managers populate `~/.config/hypr` entirely via symlinks) but guarding against
/// cycles via a canonicalized-path visited set, plus a depth cap as a second safety net.
fn keybinding_files(dir: &Path, depth_remaining: u32, visited: &mut HashSet<PathBuf>) -> Vec<PathBuf> {
    if depth_remaining == 0 {
        return Vec::new();
    }
    let Ok(canonical) = dir.canonicalize() else {
        return Vec::new();
    };
    if !visited.insert(canonical) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut files = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            files.extend(keybinding_files(&path, depth_remaining - 1, visited));
        } else if matches!(path.extension().and_then(|ext| ext.to_str()), Some("conf") | Some("lua")) {
            files.push(path);
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("hyprbind-discover-test-{name}-{}", std::process::id()))
    }

    #[test]
    fn finds_a_conf_with_binds_nested_a_few_levels_deep() {
        let root = scratch_dir("nested");
        let nested = root.join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        let file = nested.join("keybinds.conf");
        fs::write(&file, "bind = SUPER, Q, killactive\n").unwrap();

        assert_eq!(discover(&root), Some(file));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn picks_the_file_with_the_most_binds() {
        let root = scratch_dir("most-binds");
        fs::create_dir_all(&root).unwrap();
        let small = root.join("small.conf");
        let big = root.join("big.conf");
        fs::write(&small, "bind = SUPER, Q, killactive\n").unwrap();
        fs::write(
            &big,
            "bind = SUPER, Q, killactive\nbind = SUPER, W, exec, foo\nbind = SUPER, E, exec, bar\n",
        )
        .unwrap();

        assert_eq!(discover(&root), Some(big));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn finds_a_lua_keybindings_file() {
        let root = scratch_dir("lua");
        fs::create_dir_all(&root).unwrap();
        let file = root.join("default.lua");
        fs::write(
            &file,
            "hl.bind(\"CTRL + ALT + T\", hl.dsp.exec_cmd(\"x\"), { description = \"y\" })\n",
        )
        .unwrap();

        assert_eq!(discover(&root), Some(file));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn ignores_conf_files_with_zero_binds() {
        let root = scratch_dir("zero-binds");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("empty.conf"), "# just a comment\n").unwrap();

        assert_eq!(discover(&root), None);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn returns_none_for_a_missing_root() {
        let missing = scratch_dir("missing-and-hopefully-absent");
        assert_eq!(discover(&missing), None);
    }

    #[test]
    fn terminates_on_a_directory_symlink_cycle() {
        let root = scratch_dir("cycle");
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("keybinds.conf"), "bind = SUPER, Q, killactive\n").unwrap();

        // sub/loop -> root, forming a cycle that visited-set tracking must break.
        #[cfg(unix)]
        std::os::unix::fs::symlink(&root, sub.join("loop")).unwrap();

        assert_eq!(discover(&root), Some(sub.join("keybinds.conf")));

        fs::remove_dir_all(&root).unwrap();
    }
}
