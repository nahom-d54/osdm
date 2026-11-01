use crate::{Error, MediaUrl};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HlsRenditionKind {
    Audio,
    Subtitle,
}

/// Metadata is untrusted and may contain signed URLs; deliberately no Debug.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HlsRendition {
    pub kind: HlsRenditionKind,
    pub group_id: String,
    pub name: String,
    pub language: Option<String>,
    pub source: Option<MediaUrl>,
    pub default: bool,
    pub autoselect: bool,
    pub forced: bool,
}

pub(crate) fn parse_rendition(base: &MediaUrl, mut text: &str) -> Result<HlsRendition, Error> {
    let mut fields = std::collections::HashMap::new();
    while !text.is_empty() {
        let (key, rest) = text.split_once('=').ok_or(Error::Invalid)?;
        let quoted = rest.starts_with('"');
        let (value, tail) = if let Some(rest) = rest.strip_prefix('"') {
            rest.split_once('"').ok_or(Error::Invalid)?
        } else {
            rest.find(',')
                .map_or((rest, ""), |index| (&rest[..index], &rest[index..]))
        };
        if !matches!(
            key,
            "TYPE"
                | "GROUP-ID"
                | "NAME"
                | "LANGUAGE"
                | "URI"
                | "DEFAULT"
                | "AUTOSELECT"
                | "FORCED"
                | "CHANNELS"
                | "CHARACTERISTICS"
        ) {
            return Err(Error::Unsupported);
        }
        if value.is_empty()
            || value.chars().any(char::is_control)
            || fields.insert(key, (value, quoted)).is_some()
        {
            return Err(Error::Invalid);
        }
        if tail.is_empty() {
            break;
        }
        text = tail
            .strip_prefix(',')
            .filter(|tail| !tail.is_empty())
            .ok_or(Error::Invalid)?;
    }
    let required = |key| -> Result<String, Error> {
        let (value, quoted) = fields.get(key).ok_or(Error::Invalid)?;
        if !quoted || value.len() > 256 {
            return Err(Error::Invalid);
        }
        Ok((*value).to_owned())
    };
    let flag = |key| -> Result<bool, Error> {
        match fields.get(key) {
            None | Some(("NO", false)) => Ok(false),
            Some(("YES", false)) => Ok(true),
            _ => Err(Error::Invalid),
        }
    };
    let kind = match fields.get("TYPE") {
        Some(("AUDIO", false)) => HlsRenditionKind::Audio,
        Some(("SUBTITLES", false)) => HlsRenditionKind::Subtitle,
        _ => return Err(Error::Unsupported),
    };
    let source = fields
        .get("URI")
        .map(|(value, quoted)| {
            if !quoted {
                return Err(Error::Invalid);
            }
            crate::resolve(base, value)
        })
        .transpose()?;
    let language = fields
        .get("LANGUAGE")
        .map(|_| required("LANGUAGE"))
        .transpose()?;
    let default = flag("DEFAULT")?;
    let autoselect = flag("AUTOSELECT")?;
    let forced = flag("FORCED")?;
    if default && !autoselect
        || forced && kind != HlsRenditionKind::Subtitle
        || source.is_none() && kind == HlsRenditionKind::Subtitle
    {
        return Err(Error::Invalid);
    }
    for key in ["CHANNELS", "CHARACTERISTICS"] {
        if fields.get(key).is_some_and(|(_, quoted)| !quoted) {
            return Err(Error::Invalid);
        }
    }
    Ok(HlsRendition {
        kind,
        group_id: required("GROUP-ID")?,
        name: required("NAME")?,
        language,
        source,
        default,
        autoselect,
        forced,
    })
}

/// Signed difference in the wrapping 33-bit MPEG transport clock.
pub fn mpegts_clock_delta(value: u64, reference: u64) -> Result<i64, Error> {
    const MODULUS: u64 = 1 << 33;
    if value >= MODULUS || reference >= MODULUS {
        return Err(Error::Invalid);
    }
    Ok(((value + MODULUS - reference + MODULUS / 2) % MODULUS) as i64 - (MODULUS / 2) as i64)
}

/// Inspect only a caller-bounded TS prefix. No demuxer or network access.
pub fn mpegts_start_pts(bytes: &[u8], audio: bool) -> Result<u64, Error> {
    if bytes.len() > crate::MAX_MANIFEST_BYTES {
        return Err(Error::Limit);
    }
    for packet in bytes.as_chunks::<188>().0 {
        if packet[0] != 0x47 || packet[1] & 0x80 != 0 || packet[3] & 0xc0 != 0 {
            return Err(Error::Invalid);
        }
        if packet[1] & 0x40 == 0 || packet[3] & 0x10 == 0 {
            continue;
        }
        let offset = if packet[3] & 0x20 != 0 {
            5usize + packet[4] as usize
        } else {
            4
        };
        let Some(pes) = packet.get(offset..) else {
            return Err(Error::Invalid);
        };
        if pes.len() < 14 || pes[..3] != [0, 0, 1] {
            continue;
        }
        let matches = if audio {
            (0xc0..=0xdf).contains(&pes[3]) || pes[3] == 0xbd
        } else {
            (0xe0..=0xef).contains(&pes[3])
        };
        if !matches {
            continue;
        }
        if pes[7] & 0x80 == 0 || pes[8] < 5 {
            return Err(Error::Unsupported);
        }
        let pts = &pes[9..14];
        if pts[0] & 1 == 0 || pts[2] & 1 == 0 || pts[4] & 1 == 0 {
            return Err(Error::Invalid);
        }
        return Ok(((pts[0] as u64 & 0x0e) << 29)
            | ((pts[1] as u64) << 22)
            | ((pts[2] as u64 & 0xfe) << 14)
            | ((pts[3] as u64) << 7)
            | ((pts[4] as u64) >> 1));
    }
    Err(Error::Unsupported)
}

struct VttSegment {
    local: u64,
    clock: u64,
    cues: Vec<(u64, u64, String)>,
}

fn parse_vtt(body: &[u8]) -> Result<VttSegment, Error> {
    if body.len() > crate::MAX_MANIFEST_BYTES {
        return Err(Error::Limit);
    }
    let text = std::str::from_utf8(body)
        .map_err(|_| Error::Invalid)?
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n");
    if text
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        return Err(Error::Invalid);
    }
    if text.lines().any(|line| line.len() > 8192) {
        return Err(Error::Limit);
    }
    let mut blocks = text.split("\n\n").filter(|block| !block.trim().is_empty());
    let mut header = blocks.next().ok_or(Error::Invalid)?.lines();
    if header.next() != Some("WEBVTT") {
        return Err(Error::Invalid);
    }
    let mut mapping = None;
    for line in header {
        let fields = line
            .strip_prefix("X-TIMESTAMP-MAP=")
            .ok_or(Error::Unsupported)?;
        if mapping.is_some() {
            return Err(Error::Invalid);
        }
        let mut local = None;
        let mut clock = None;
        for field in fields.split(',') {
            if let Some(value) = field.strip_prefix("LOCAL:") {
                if local.is_some() {
                    return Err(Error::Invalid);
                }
                local = Some(crate::subtitles::timestamp(
                    value,
                    crate::SubtitleFormat::WebVtt,
                )?);
            } else if let Some(value) = field.strip_prefix("MPEGTS:") {
                if clock.is_some() {
                    return Err(Error::Invalid);
                }
                let value = crate::unsigned(value)?;
                if value >= 1 << 33 {
                    return Err(Error::Invalid);
                }
                clock = Some(value);
            } else {
                return Err(Error::Unsupported);
            }
        }
        mapping = Some((local.ok_or(Error::Invalid)?, clock.ok_or(Error::Invalid)?));
    }
    let (local, clock) = mapping.unwrap_or((0, 0));
    let mut cues = Vec::new();
    for block in blocks {
        let mut lines = block.trim_matches('\n').lines();
        let first = lines.next().ok_or(Error::Invalid)?;
        let timing = if first.contains(" --> ") {
            first
        } else {
            if first.starts_with("NOTE") || matches!(first, "STYLE" | "REGION") {
                return Err(Error::Unsupported);
            }
            lines.next().ok_or(Error::Invalid)?
        };
        let (start, end) = timing.split_once(" --> ").ok_or(Error::Invalid)?;
        let start = crate::subtitles::timestamp(start, crate::SubtitleFormat::WebVtt)?;
        let end = crate::subtitles::timestamp(end, crate::SubtitleFormat::WebVtt)?;
        let payload = lines.collect::<Vec<_>>().join("\n");
        if start >= end || payload.trim().is_empty() {
            return Err(Error::Invalid);
        }
        if cues.len() >= crate::MAX_ENTRIES {
            return Err(Error::Limit);
        }
        cues.push((start, end, payload));
    }
    Ok(VttSegment { local, clock, cues })
}

pub fn validate_hls_webvtt_segment(body: &[u8]) -> Result<(), Error> {
    parse_vtt(body).map(|_| ())
}

/// Normalize HLS WebVTT onto the primary TS presentation clock, deduplicating
/// repeated boundary cues without shifting each subtitle segment independently.
pub fn merge_hls_webvtt(segments: &[Vec<u8>], primary_pts: u64) -> Result<Vec<u8>, Error> {
    if segments.is_empty()
        || segments.len() > crate::MAX_ENTRIES
        || segments.iter().map(Vec::len).sum::<usize>() > crate::MAX_MANIFEST_BYTES
    {
        return Err(Error::Limit);
    }
    let mut cues = std::collections::BTreeSet::new();
    for bytes in segments {
        let segment = parse_vtt(bytes)?;
        let shift =
            mpegts_clock_delta(segment.clock, primary_pts)? as i128 / 90 - segment.local as i128;
        for (start, end, text) in segment.cues {
            let start = start as i128 + shift;
            let end = end as i128 + shift;
            if end <= 0 {
                continue;
            }
            if end > 7 * 24 * 3600 * 1000 {
                return Err(Error::Unsupported);
            }
            cues.insert((start.max(0) as u64, end as u64, text));
            if cues.len() > crate::MAX_ENTRIES {
                return Err(Error::Limit);
            }
        }
    }
    let stamp = |time: u64| {
        format!(
            "{:02}:{:02}:{:02}.{:03}",
            time / 3_600_000,
            time / 60_000 % 60,
            time / 1000 % 60,
            time % 1000
        )
    };
    let mut output = String::from("WEBVTT\n\n");
    for (start, end, text) in cues {
        output.push_str(&format!(
            "{} --> {}\n{}\n\n",
            stamp(start),
            stamp(end),
            text
        ));
        if output.len() > crate::MAX_MANIFEST_BYTES {
            return Err(Error::Limit);
        }
    }
    crate::validate_subtitles(crate::SubtitleFormat::WebVtt, output.as_bytes())?;
    Ok(output.into_bytes())
}
