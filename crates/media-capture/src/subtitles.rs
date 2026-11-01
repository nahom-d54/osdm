use crate::{Error, MAX_ENTRIES, MAX_LINE_BYTES, MAX_MANIFEST_BYTES};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubtitleFormat {
    Srt,
    WebVtt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleInfo {
    pub format: SubtitleFormat,
    pub cue_count: usize,
    pub end_milliseconds: u64,
}

pub(crate) fn timestamp(value: &str, format: SubtitleFormat) -> Result<u64, Error> {
    let separator = if format == SubtitleFormat::Srt {
        ','
    } else {
        '.'
    };
    let (time, millis) = value.split_once(separator).ok_or(Error::Invalid)?;
    if millis.len() != 3 || !millis.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Invalid);
    }
    let fields: Vec<_> = time.split(':').collect();
    if !(fields.len() == 3 || fields.len() == 2 && format == SubtitleFormat::WebVtt) {
        return Err(Error::Invalid);
    }
    if fields
        .iter()
        .any(|v| v.len() < 2 || !v.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(Error::Invalid);
    }
    let seconds = crate::unsigned(fields[fields.len() - 1])?;
    let minutes = crate::unsigned(fields[fields.len() - 2])?;
    if seconds >= 60
        || minutes >= 60
        || fields[fields.len() - 1].len() != 2
        || fields[fields.len() - 2].len() != 2
    {
        return Err(Error::Invalid);
    }
    let hours = if fields.len() == 3 {
        crate::unsigned(fields[0])?
    } else {
        0
    };
    hours
        .checked_mul(3600)
        .and_then(|v| v.checked_add(minutes * 60 + seconds))
        .and_then(|v| v.checked_mul(1000))
        .and_then(|v| v.checked_add(crate::unsigned(millis).ok()?))
        .ok_or(Error::Invalid)
}

/// Validate a bounded plain SRT/WebVTT cue document, not arbitrary timed text.
/// Cue identifiers are preserved in the original bytes; rendering is not performed.
pub fn validate_subtitles(format: SubtitleFormat, body: &[u8]) -> Result<SubtitleInfo, Error> {
    if body.len() > MAX_MANIFEST_BYTES {
        return Err(Error::Limit);
    }
    let text = std::str::from_utf8(body)
        .map_err(|_| Error::Invalid)?
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n");
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(Error::Invalid);
    }
    if text.lines().any(|line| line.len() > MAX_LINE_BYTES) {
        return Err(Error::Limit);
    }
    let mut blocks = text.split("\n\n").filter(|b| !b.trim().is_empty());
    if format == SubtitleFormat::WebVtt && blocks.next() != Some("WEBVTT") {
        return Err(Error::Invalid);
    }
    let mut cue_count = 0;
    let mut end_milliseconds = 0;
    for block in blocks {
        let mut lines = block.trim_matches('\n').lines();
        let first = lines.next().ok_or(Error::Invalid)?;
        let timing = if format == SubtitleFormat::Srt {
            if crate::positive(first)? != cue_count as u64 + 1 {
                return Err(Error::Invalid);
            }
            lines.next().ok_or(Error::Invalid)?
        } else if first.contains(" --> ") {
            first
        } else {
            if matches!(first, "STYLE" | "REGION")
                || first.starts_with("NOTE")
                || first.contains("-->")
            {
                return Err(Error::Unsupported);
            }
            lines.next().ok_or(Error::Invalid)?
        };
        let (start, end) = timing.split_once(" --> ").ok_or(Error::Invalid)?;
        let start = timestamp(start, format)?;
        let end = timestamp(end, format)?;
        if start >= end || !lines.any(|l| !l.trim().is_empty()) {
            return Err(Error::Invalid);
        }
        cue_count += 1;
        if cue_count > MAX_ENTRIES {
            return Err(Error::Limit);
        }
        end_milliseconds = end_milliseconds.max(end);
    }
    if cue_count == 0 {
        return Err(Error::Invalid);
    }
    Ok(SubtitleInfo {
        format,
        cue_count,
        end_milliseconds,
    })
}
