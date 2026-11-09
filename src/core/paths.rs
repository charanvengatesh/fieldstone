//! Where the game lives on disk. Content (`assets/`, `mods/`) sits next to the executable in a
//! release and in the repo root during development; player files (`settings.ron`, `saves/`)
//! go beside the content when that folder is writable, else into the OS config directory.

use std::path::{Path, PathBuf};

/// Directory holding `assets/packs`. Checks the current directory first (cargo run), then the
/// executable's folder and its parents (release zip, macOS bundle).
pub fn root() -> PathBuf {
    let has_content = |p: &Path| p.join("assets").join("packs").is_dir();
    if let Ok(cwd) = std::env::current_dir()
        && has_content(&cwd)
    {
        return cwd;
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(Path::to_path_buf);
        while let Some(d) = dir {
            if has_content(&d) {
                return d;
            }
            dir = d.parent().map(Path::to_path_buf);
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Directory for `settings.ron` and `saves/`: the content root when writable, otherwise the
/// per-user config directory.
pub fn user_dir() -> PathBuf {
    let root = root();
    if writable(&root) {
        return root;
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                let h = PathBuf::from(h);
                if cfg!(target_os = "macos") {
                    h.join("Library").join("Application Support")
                } else {
                    h.join(".config")
                }
            })
        })
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("Fieldstone");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(".fieldstone-write-test");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

pub fn settings_file() -> PathBuf {
    user_dir().join("settings.ron")
}

pub fn save_file() -> PathBuf {
    user_dir().join("saves").join("iso.ron")
}
