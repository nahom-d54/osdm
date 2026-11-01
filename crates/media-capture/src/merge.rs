use crate::{MAX_ENTRIES, MAX_MANIFEST_BYTES, SubtitleFormat, validate_subtitles};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentContainer {
    MpegTs,
    FragmentedMp4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputContainer {
    Mp4,
    Matroska,
}

/// Local paths intentionally have no Debug/serde implementation. Input files must
/// be engine-owned, immutable verified segments, not user-controlled playlists.
pub struct LocalTrack {
    /// Initialization first for fragmented MP4, then ordered media fragments.
    pub files: Vec<PathBuf>,
    pub container: SegmentContainer,
}

pub struct LocalSubtitle {
    pub path: PathBuf,
    pub format: SubtitleFormat,
    /// Optional BCP-47-style language label; never interpreted as arguments.
    pub language: Option<String>,
}

pub struct MergePlan {
    pub tracks: Vec<LocalTrack>,
    pub subtitles: Vec<LocalSubtitle>,
    pub output: OutputContainer,
    /// Aggregate staging budget, including subtitles (must be nonzero).
    pub max_input_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MergeError {
    #[error("invalid local media merge plan")]
    Invalid,
    #[error("local media merge exceeds its budget")]
    Limit,
    #[error("local media merge cancelled")]
    Cancelled,
    #[error("local media merge I/O failed")]
    Io,
    #[error("configured media remuxer failed")]
    Remux,
}

/// An unverified temporary artifact. Dropping it removes the file. This is not a
/// published download: the engine must hash these exact bytes and publish safely.
pub struct MergeArtifact {
    output: tempfile::NamedTempFile,
}

impl MergeArtifact {
    pub fn path(&self) -> &Path {
        self.output.path()
    }
}

fn local_absolute(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        matches!(path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
    }
    #[cfg(not(windows))]
    {
        true
    }
}

impl MergePlan {
    pub fn validate(&self) -> Result<(), MergeError> {
        let count: usize = self.tracks.iter().map(|t| t.files.len()).sum();
        if self.tracks.is_empty()
            || self.tracks.len() > 16
            || self.subtitles.len() > 16
            || self.tracks.iter().any(|t| t.files.is_empty())
            || count > MAX_ENTRIES
            || self.max_input_bytes == 0
        {
            return Err(MergeError::Invalid);
        }
        if self
            .tracks
            .iter()
            .flat_map(|t| &t.files)
            .chain(self.subtitles.iter().map(|s| &s.path))
            .any(|p| !local_absolute(p))
        {
            return Err(MergeError::Invalid);
        }
        if self
            .subtitles
            .iter()
            .filter_map(|s| s.language.as_deref())
            .any(|s| {
                s.is_empty()
                    || s.len() > 35
                    || !s.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
            })
        {
            return Err(MergeError::Invalid);
        }
        Ok(())
    }
}

async fn stage(
    source: &Path,
    destination: &mut tokio::fs::File,
    remaining: &mut u64,
    cancel: &CancellationToken,
) -> Result<(), MergeError> {
    // Reject symlinks and special files; opened-file metadata is checked as well.
    if !tokio::fs::symlink_metadata(source)
        .await
        .map_err(|_| MergeError::Io)?
        .file_type()
        .is_file()
    {
        return Err(MergeError::Invalid);
    }
    let mut input = tokio::fs::File::open(source)
        .await
        .map_err(|_| MergeError::Io)?;
    let metadata = input.metadata().await.map_err(|_| MergeError::Io)?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(MergeError::Invalid);
    }
    if metadata.len() > *remaining {
        return Err(MergeError::Limit);
    }
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = tokio::select! { biased; _ = cancel.cancelled() => return Err(MergeError::Cancelled), result = input.read(&mut buffer) => result.map_err(|_| MergeError::Io)? };
        if n == 0 {
            break;
        }
        *remaining = remaining.checked_sub(n as u64).ok_or(MergeError::Limit)?;
        tokio::select! { biased; _ = cancel.cancelled() => return Err(MergeError::Cancelled), result = destination.write_all(&buffer[..n]) => result.map_err(|_| MergeError::Io)? };
    }
    Ok(())
}

/// Cancellable local-only remux. `ffmpeg` must be an explicitly configured absolute
/// executable path. No PATH lookup, shell, network inputs, concat scripts, stderr
/// logging, or destination publication. Staging lives in a private temp directory.
pub async fn remux_local(
    ffmpeg: &Path,
    plan: &MergePlan,
    cancel: &CancellationToken,
) -> Result<MergeArtifact, MergeError> {
    plan.validate()?;
    if cancel.is_cancelled() {
        return Err(MergeError::Cancelled);
    }
    let (mut command, output, _dir) = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(MergeError::Cancelled),
        result = prepare(ffmpeg, plan, cancel) => result?,
    };
    if cancel.is_cancelled() {
        return Err(MergeError::Cancelled);
    }
    let max_output = plan
        .max_input_bytes
        .saturating_mul(2)
        .saturating_add(16 * 1024 * 1024);
    let status = run_command(&mut command, cancel, Some((output.path(), max_output))).await?;
    if !status.success()
        || output
            .as_file()
            .metadata()
            .map_err(|_| MergeError::Io)?
            .len()
            == 0
    {
        return Err(MergeError::Remux);
    }
    Ok(MergeArtifact { output })
}

async fn run_command(
    command: &mut Command,
    cancel: &CancellationToken,
    output: Option<(&Path, u64)>,
) -> Result<std::process::ExitStatus, MergeError> {
    command.kill_on_drop(true);
    let mut child = command.spawn().map_err(|_| MergeError::Remux)?;
    let deadline = tokio::time::sleep(std::time::Duration::from_secs(3600));
    tokio::pin!(deadline);
    let monitor = async {
        if let Some((path, cap)) = output {
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(100));
            loop {
                tick.tick().await;
                if tokio::fs::metadata(path)
                    .await
                    .map_err(|_| MergeError::Io)?
                    .len()
                    > cap
                {
                    return Err::<(), _>(MergeError::Limit);
                }
            }
        } else {
            std::future::pending::<Result<(), MergeError>>().await
        }
    };
    tokio::pin!(monitor);
    let status = tokio::select! {
        biased;
        _ = cancel.cancelled() => { let _ = child.kill().await; let _ = child.wait().await; return Err(MergeError::Cancelled); }
        _ = &mut deadline => { let _ = child.kill().await; let _ = child.wait().await; return Err(MergeError::Limit); }
        result = &mut monitor => { let _ = child.kill().await; let _ = child.wait().await; return Err(result.err().unwrap_or(MergeError::Limit)); }
        status = child.wait() => status.map_err(|_| MergeError::Remux)?,
    };
    if let Some((path, cap)) = output {
        let metadata = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(MergeError::Cancelled),
            result = tokio::fs::metadata(path) => result.map_err(|_| MergeError::Io)?,
        };
        if metadata.len() > cap {
            return Err(MergeError::Limit);
        }
    }
    Ok(status)
}

async fn prepare(
    ffmpeg: &Path,
    plan: &MergePlan,
    cancel: &CancellationToken,
) -> Result<(Command, tempfile::NamedTempFile, tempfile::TempDir), MergeError> {
    if !local_absolute(ffmpeg)
        || !tokio::fs::metadata(ffmpeg)
            .await
            .map_err(|_| MergeError::Io)?
            .is_file()
    {
        return Err(MergeError::Invalid);
    }
    let dir = tempfile::tempdir().map_err(|_| MergeError::Io)?;
    let output = tempfile::NamedTempFile::new().map_err(|_| MergeError::Io)?;
    let mut command = Command::new(ffmpeg);
    command
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.args([
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-max_alloc",
        "67108864",
        "-threads",
        "1",
        "-y",
    ]);
    let mut remaining = plan.max_input_bytes;
    for (index, track) in plan.tracks.iter().enumerate() {
        let path = dir.path().join(format!("track-{index}"));
        let mut staged = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
            .map_err(|_| MergeError::Io)?;
        for file in &track.files {
            stage(file, &mut staged, &mut remaining, cancel).await?;
        }
        staged.flush().await.map_err(|_| MergeError::Io)?;
        drop(staged);
        command.args([
            "-protocol_whitelist",
            "file",
            "-f",
            match track.container {
                SegmentContainer::MpegTs => "mpegts",
                SegmentContainer::FragmentedMp4 => "mov",
            },
        ]);
        if track.container == SegmentContainer::FragmentedMp4 {
            command.args(["-enable_drefs", "0", "-use_absolute_path", "0"]);
        }
        command.arg("-i").arg(path);
    }
    for (index, subtitle) in plan.subtitles.iter().enumerate() {
        let path = dir.path().join(format!("subtitle-{index}"));
        let mut staged = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
            .map_err(|_| MergeError::Io)?;
        let mut subtitle_budget = remaining.min(MAX_MANIFEST_BYTES as u64);
        let before = subtitle_budget;
        stage(&subtitle.path, &mut staged, &mut subtitle_budget, cancel).await?;
        remaining -= before - subtitle_budget;
        staged.flush().await.map_err(|_| MergeError::Io)?;
        drop(staged);
        let bytes = tokio::fs::read(&path).await.map_err(|_| MergeError::Io)?;
        validate_subtitles(subtitle.format, &bytes).map_err(|_| MergeError::Invalid)?;
        command.args([
            "-protocol_whitelist",
            "file",
            "-f",
            match subtitle.format {
                SubtitleFormat::Srt => "srt",
                SubtitleFormat::WebVtt => "webvtt",
            },
        ]);
        command.arg("-i").arg(path);
    }
    for index in 0..plan.tracks.len() + plan.subtitles.len() {
        command.arg("-map").arg(index.to_string());
    }
    command.args(["-map_metadata", "-1", "-map_chapters", "-1", "-c", "copy"]);
    if !plan.subtitles.is_empty() {
        command.args([
            "-c:s",
            match plan.output {
                OutputContainer::Mp4 => "mov_text",
                OutputContainer::Matroska => "srt",
            },
        ]);
    }
    for (index, subtitle) in plan.subtitles.iter().enumerate() {
        if let Some(language) = &subtitle.language {
            command
                .arg(format!("-metadata:s:s:{index}"))
                .arg(format!("language={language}"));
        }
    }
    command.args([
        "-f",
        match plan.output {
            OutputContainer::Mp4 => "mp4",
            OutputContainer::Matroska => "matroska",
        },
    ]);
    command.arg(output.path());
    Ok((command, output, dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_kills_and_reaps_a_running_child() {
        #[cfg(windows)]
        let mut command = {
            let root = std::env::var_os("WINDIR").expect("Windows system directory");
            let mut command = Command::new(PathBuf::from(root).join("System32/ping.exe"));
            command.args(["-n", "60", "127.0.0.1"]);
            command
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut c = Command::new("/bin/sleep");
            c.arg("60");
            c
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let cancel = CancellationToken::new();
        let signal = cancel.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            signal.cancel();
        });
        let start = std::time::Instant::now();
        assert_eq!(
            run_command(&mut command, &cancel, None).await,
            Err(MergeError::Cancelled)
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
        thread.join().unwrap();
    }

    #[tokio::test]
    async fn output_guard_kills_and_reaps_a_child_before_returning_limit() {
        #[cfg(windows)]
        let mut command = {
            let root = std::env::var_os("WINDIR").expect("Windows system directory");
            let mut command = Command::new(PathBuf::from(root).join("System32/ping.exe"));
            command.args(["-n", "60", "127.0.0.1"]);
            command
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut command = Command::new("/bin/sleep");
            command.arg("60");
            command
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"oversized-output-fixture").unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            run_command(
                &mut command,
                &CancellationToken::new(),
                Some((file.path(), 1)),
            ),
        )
        .await
        .unwrap();
        assert_eq!(result, Err(MergeError::Limit));
    }
}
