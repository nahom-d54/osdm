//! Explicitly opted-in desktop actions. Never run a shell or infer a scanner.
use crate::{Reply, settings::DesktopPreferences};
use std::path::Path;
use tokio::io::AsyncReadExt;

pub async fn scan(settings: &DesktopPreferences, file: &Path) -> Reply<()> {
    if !settings.scanner_enabled {
        return Ok(());
    }
    let mut command = scanner_command(settings, file)?;
    let status = tokio::time::timeout(std::time::Duration::from_secs(300), command.status())
        .await
        .map_err(|_| "scanner timed out after five minutes")?
        .map_err(|_| "could not launch the configured scanner")?;
    if !status.success() {
        return Err(format!(
            "scanner exited unsuccessfully (code {:?}); this is not a malware verdict",
            status.code()
        ));
    }
    Ok(())
}

fn scanner_command(settings: &DesktopPreferences, file: &Path) -> Reply<tokio::process::Command> {
    let executable = Path::new(&settings.scanner_executable);
    #[cfg(windows)]
    if !executable.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("com")
    }) {
        return Err(
            "choose a native .exe or .com scanner; batch scripts would implicitly launch a shell"
                .into(),
        );
    }
    if !executable.is_absolute()
        || !executable.is_file()
        || !settings.scanner_arguments.iter().any(|arg| arg == "{file}")
    {
        return Err(
            "scanner requires an existing absolute executable and a {file} argument".into(),
        );
    }
    let mut command = tokio::process::Command::new(executable);
    for argument in &settings.scanner_arguments {
        if argument == "{file}" {
            command.arg(file);
        } else {
            command.arg(argument);
        }
    }
    command
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    Ok(command)
}

pub async fn sound(path: &Path) -> Reply<()> {
    if !path.is_absolute() {
        return Err("sound file must be absolute".into());
    }
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|_| "cannot open sound file")?;
    if file
        .metadata()
        .await
        .map_err(|_| "cannot read sound file")?
        .len()
        > 10 * 1024 * 1024
    {
        return Err("sound file exceeds 10 MiB".into());
    }
    let mut header = [0u8; 12];
    file.read_exact(&mut header)
        .await
        .map_err(|_| "invalid WAV file")?;
    if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
        return Err("choose a RIFF/WAVE sound file".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        tauri::async_runtime::spawn_blocking(move || {
            #[link(name = "winmm")]
            unsafe extern "system" {
                fn PlaySoundW(sound: *const u16, module: isize, flags: u32) -> i32;
            }
            // Synchronous bounded WAV playback keeps the string alive until return.
            // No alias, executable, shell, or sound registry lookup is used.
            if unsafe { PlaySoundW(wide.as_ptr(), 0, 0x00020000 | 0x00000002) } == 0 {
                Err("Windows could not play this WAV file".into())
            } else {
                Ok(())
            }
        })
        .await
        .map_err(|_| "sound task failed")?
    }
    #[cfg(not(windows))]
    {
        Err("custom event sound playback is currently supported on Windows only".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanner_passes_literal_arguments_and_requires_explicit_file_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("scanner.exe");
        std::fs::write(&executable, b"not executed").unwrap();
        let file = dir.path().join("name & $(not-a-command).bin");
        let mut settings = DesktopPreferences {
            scanner_enabled: true,
            scanner_executable: executable.to_string_lossy().into_owned(),
            scanner_arguments: vec!["--scan".into(), "{file}".into()],
            ..Default::default()
        };
        let command = scanner_command(&settings, &file).unwrap();
        let args: Vec<_> = command.as_std().get_args().collect();
        assert_eq!(args, vec![std::ffi::OsStr::new("--scan"), file.as_os_str()]);
        settings.scanner_arguments = vec!["prefix={file}".into()];
        assert!(scanner_command(&settings, &file).is_err());
    }

    #[tokio::test]
    async fn sound_rejects_non_wav_and_oversized_files_without_playing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("invalid.wav");
        tokio::fs::write(&file, b"not a wave file").await.unwrap();
        assert!(sound(&file).await.unwrap_err().contains("RIFF"));
        let oversized = tokio::fs::File::create(&file).await.unwrap();
        oversized.set_len(10 * 1024 * 1024 + 1).await.unwrap();
        assert!(sound(&file).await.unwrap_err().contains("10 MiB"));
        assert!(sound(Path::new("relative.wav")).await.is_err());
    }
}
