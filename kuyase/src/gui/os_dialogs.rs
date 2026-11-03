use protocol::normalize_board_slug;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn discover_images_in_folder(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if (name.ends_with(".ext4") || name.ends_with("-system.img"))
            && !name.ends_with("-orig.ext4")
            && !name.ends_with("-data.ext4")
        {
            let stem = name
                .trim_end_matches("-system.ext4")
                .trim_end_matches(".ext4")
                .trim_end_matches("-system.img");
            found.push((normalize_board_slug(stem), path));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

pub fn run_os_file_dialog(start_dir: &Path) -> Option<PathBuf> {
    run_os_custom_file_dialog(
        start_dir,
        "Выберите образ (.ext4) или архив дампа",
        "Образы и дампы (*.ext4;*.img;*.gz;*.tgz;*.tar;*.zip;*.ubi;*.squashfs)|*.ext4;*.img;*.gz;*.tgz;*.tar;*.zip;*.ubi;*.squashfs|Все файлы (*.*)|*.*",
    )
}

pub fn run_os_custom_file_dialog(start_dir: &Path, title: &str, filter: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let init = start_dir.display().to_string().replace('\'', "''");
        let safe_title = title.replace('\'', "''");
        let safe_filter = filter.replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $d = New-Object System.Windows.Forms.OpenFileDialog; \
             $d.Title = '{}'; \
             if (Test-Path '{}') {{ $d.InitialDirectory = '{}' }}; \
             $d.Filter = '{}'; \
             if ($d.ShowDialog() -eq 'OK') {{ [Console]::Out.Write($d.FileName) }}",
            safe_title, init, init, safe_filter
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-STA", "-Command", &script])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(PathBuf::from(s));
        }
        None
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (start_dir, filter);
        let out = Command::new("zenity")
            .args(["--file-selection", &format!("--title={}", title)])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(PathBuf::from(s));
        }
        None
    }
}

pub fn run_os_folder_dialog(start_dir: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let init = start_dir.display().to_string().replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $d = New-Object System.Windows.Forms.FolderBrowserDialog; \
             $d.Description = 'Выберите папку с гостевыми образами (*-system.ext4)'; \
             if (Test-Path '{}') {{ $d.SelectedPath = '{}' }}; \
             if ($d.ShowDialog() -eq 'OK') {{ [Console]::Out.Write($d.SelectedPath) }}",
            init, init
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-STA", "-Command", &script])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(PathBuf::from(s));
        }
        None
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = start_dir;
        let out = Command::new("zenity")
            .args([
                "--file-selection",
                "--directory",
                "--title=Выберите папку с образами",
            ])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(PathBuf::from(s));
        }
        None
    }
}
