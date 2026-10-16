//! Small OS differences kept in one place: home folder, path canonicalization
//! without Windows verbatim prefixes, and spawning helpers that keep console
//! windows from flashing up behind the GUI on Windows.

use std::path::{Path, PathBuf};

/// The user's home folder (`HOME`, or `USERPROFILE` on Windows).
pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Same folder Tauri's `app_data_dir()` picks, for tools that run without Tauri.
pub fn default_data_dir() -> PathBuf {
    const ID: &str = "com.mangaai.studio";
    if cfg!(windows) {
        let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| home_dir().join("AppData/Roaming"));
        appdata.join(ID)
    } else if cfg!(target_os = "macos") {
        home_dir().join("Library/Application Support").join(ID)
    } else {
        let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home_dir().join(".local/share"));
        data.join(ID)
    }
}

/// `canonicalize`, but on Windows returns `C:\x` instead of `\\?\C:\x` when that is
/// equivalent. Verbatim paths confuse Python, FreeType and anything shown to the user.
pub fn canonicalize(p: &Path) -> std::io::Result<PathBuf> {
    p.canonicalize().map(simplify)
}

pub fn simplify(p: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            let b = rest.as_bytes();
            // Only plain drive paths short enough to need no verbatim form.
            if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\' && rest.len() < 260 {
                return PathBuf::from(rest);
            }
        }
    }
    p
}

/// Executable file name for this platform (`uv` → `uv.exe` on Windows).
pub fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Spawn without a console window (Windows); no-op elsewhere.
pub fn no_window(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Tie a long-lived child (AI worker, Suwayomi) to this process: on Windows it joins a job
/// object that the OS closes when the app exits for any reason, crash included, which kills
/// the child. (`kill_on_drop` only covers orderly shutdown.) No-op elsewhere.
pub fn adopt(child: &tokio::process::Child) {
    #[cfg(windows)]
    if let Some(h) = child.raw_handle() {
        if let Some(job) = job::get() {
            // SAFETY: both handles are valid for the duration of the call.
            unsafe { windows_sys::Win32::System::JobObjects::AssignProcessToJobObject(job, h as _) };
        }
    }
    #[cfg(not(windows))]
    let _ = child;
}

#[cfg(windows)]
mod job {
    use std::sync::OnceLock;
    use windows_sys::Win32::System::JobObjects::*;

    struct Job(isize);
    static JOB: OnceLock<Option<Job>> = OnceLock::new();

    pub fn get() -> Option<windows_sys::Win32::Foundation::HANDLE> {
        JOB.get_or_init(|| unsafe {
            let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if h.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                h,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            // The handle is never closed: it lives as long as the app, and closing it is the kill.
            (ok != 0).then_some(Job(h as isize))
        })
        .as_ref()
        .map(|j| j.0 as _)
    }
}

/// Names of NVIDIA GPUs (via `nvidia-smi`, which ships with the driver). Empty on macOS
/// or when there is no NVIDIA driver.
pub async fn nvidia_gpus() -> Vec<String> {
    if cfg!(target_os = "macos") {
        return vec![];
    }
    let mut cmd = tokio::process::Command::new("nvidia-smi");
    no_window(&mut cmd).args(["--query-gpu=name", "--format=csv,noheader"]).kill_on_drop(true);
    match tokio::time::timeout(std::time::Duration::from_secs(5), cmd.output()).await {
        Ok(Ok(out)) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_names() {
        assert_eq!(exe("uv"), if cfg!(windows) { "uv.exe" } else { "uv" });
    }

    #[test]
    fn canonical_paths_are_plain() {
        let d = tempfile::tempdir().unwrap();
        let c = canonicalize(d.path()).unwrap();
        assert!(!c.to_string_lossy().starts_with(r"\\?\"), "{}", c.display());
        assert!(c.is_absolute());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn adopted_children_join_the_kill_on_close_job() {
        let mut child = no_window(&mut tokio::process::Command::new("ping"))
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        adopt(&child);
        let mut inside = 0;
        // SAFETY: valid process and job handles.
        let ok = unsafe {
            windows_sys::Win32::System::JobObjects::IsProcessInJob(child.raw_handle().unwrap() as _, job::get().unwrap(), &mut inside)
        };
        child.kill().await.unwrap();
        assert!(ok != 0 && inside != 0, "child is in the app's job");
    }

    #[tokio::test]
    async fn gpu_probe_never_fails() {
        let gpus = nvidia_gpus().await; // empty without an NVIDIA driver
        assert!(gpus.iter().all(|g| !g.trim().is_empty()));
    }

    #[test]
    fn data_dir_matches_tauri_layout() {
        let d = default_data_dir();
        assert!(d.ends_with("com.mangaai.studio"));
        if cfg!(windows) {
            assert!(d.to_string_lossy().contains("Roaming"), "{}", d.display());
        }
    }

    #[cfg(windows)]
    #[test]
    fn keeps_unc_and_strips_drive() {
        assert_eq!(simplify(PathBuf::from(r"\\?\C:\a\b")), PathBuf::from(r"C:\a\b"));
        assert_eq!(simplify(PathBuf::from(r"\\?\UNC\srv\share")), PathBuf::from(r"\\?\UNC\srv\share"));
    }
}
