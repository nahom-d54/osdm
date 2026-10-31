#![doc = include_str!("../README.md")]

use url::Url;

/// Maximum accepted manifest size. Callers must also bound network reads.
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub const MAX_ENTRIES: usize = 4096;
const MAX_LINE_BYTES: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("media manifest exceeds inspection limits")]
    Limit,
    #[error("invalid HLS manifest")]
    Invalid,
    #[error("unsupported HLS feature")]
    Unsupported,
    #[error("media source must be an HTTP(S) URL without credentials or a fragment")]
    UnsafeUrl,
}

/// A secret-bearing URL. Debug output deliberately omits the entire URL.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct MediaUrl(Url);

impl std::fmt::Debug for MediaUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MediaUrl([redacted])")
    }
}

impl MediaUrl {
    /// Validate an explicit source without exposing it in errors or debug output.
    pub fn parse(value: &str) -> Result<Self, Error> {
        if value.trim() != value || value.chars().any(char::is_control) {
            return Err(Error::UnsafeUrl);
        }
        if value.len() > MAX_LINE_BYTES {
            return Err(Error::Limit);
        }
        Self::checked(Url::parse(value).map_err(|_| Error::UnsafeUrl)?)
    }

    /// Explicit access for a future engine handoff; never log this value.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn checked(url: Url) -> Result<Self, Error> {
        if url.as_str().len() > MAX_LINE_BYTES {
            return Err(Error::Limit);
        }
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::UnsafeUrl);
        }
        Ok(Self(url))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub source: MediaUrl,
    /// Peak bandwidth declared by the manifest, in bits per second.
    pub bandwidth: u64,
    pub resolution: Option<(u32, u32)>,
    pub codecs: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub source: MediaUrl,
    pub duration_seconds: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Manifest {
    /// Choices in manifest order; inspection never selects or queues a choice.
    Master { variants: Vec<Variant> },
    /// Complete, unencrypted, standalone-segment VOD only.
    Vod { segments: Vec<Segment> },
}

/// Inspect an already-bounded UTF-8 HLS document relative to its final response URL.
///
/// No network or filesystem work occurs. Unknown EXT tags fail closed rather than
/// silently discarding encryption, byte-range, rendition, or timeline semantics.
pub fn inspect_hls(base_url: &str, body: &[u8]) -> Result<Manifest, Error> {
    if body.len() > MAX_MANIFEST_BYTES || base_url.len() > MAX_LINE_BYTES {
        return Err(Error::Limit);
    }
    if base_url.trim() != base_url || base_url.chars().any(char::is_control) {
        return Err(Error::UnsafeUrl);
    }
    let base = MediaUrl::parse(base_url)?;
    let text = std::str::from_utf8(body).map_err(|_| Error::Invalid)?;
    let mut lines = text.lines();
    if lines.next() != Some("#EXTM3U") {
        return Err(Error::Invalid);
    }
    let mut variants = Vec::new();
    let mut segments = Vec::new();
    let mut pending_variant = None;
    let mut pending_duration = None;
    let mut target_duration = None;
    let mut ended = false;
    let mut version = false;
    let mut sequence = false;
    let mut playlist_type = false;

    for line in lines {
        if line.len() > MAX_LINE_BYTES {
            return Err(Error::Limit);
        }
        if line.is_empty() {
            continue;
        }
        if line.trim() != line || line.chars().any(char::is_control) {
            return Err(Error::Invalid);
        }
        if line.starts_with('#') {
            if !line.starts_with("#EXT") {
                continue;
            }
            if ended || pending_variant.is_some() || pending_duration.is_some() {
                return Err(Error::Invalid);
            }
            if let Some(value) = line.strip_prefix("#EXT-X-STREAM-INF:") {
                if !segments.is_empty() || target_duration.is_some() || sequence || playlist_type {
                    return Err(Error::Invalid);
                }
                pending_variant = Some(parse_variant(value)?);
            } else if let Some(value) = line.strip_prefix("#EXTINF:") {
                if !variants.is_empty() || target_duration.is_none() {
                    return Err(Error::Invalid);
                }
                let (duration, _) = value.split_once(',').ok_or(Error::Invalid)?;
                if duration.is_empty() || !duration.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                {
                    return Err(Error::Invalid);
                }
                let duration: f64 = duration.parse().map_err(|_| Error::Invalid)?;
                if !duration.is_finite() || duration <= 0.0 {
                    return Err(Error::Invalid);
                }
                if duration.round() > target_duration.unwrap_or(0) as f64 {
                    return Err(Error::Invalid);
                }
                pending_duration = Some(duration);
            } else if let Some(value) = line.strip_prefix("#EXT-X-TARGETDURATION:") {
                if target_duration.is_some() || !variants.is_empty() {
                    return Err(Error::Invalid);
                }
                target_duration = Some(positive(value)?);
            } else if let Some(value) = line.strip_prefix("#EXT-X-VERSION:") {
                if version {
                    return Err(Error::Invalid);
                }
                version = true;
                if positive(value)? > 3 {
                    return Err(Error::Unsupported);
                }
            } else if let Some(value) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
                if sequence || !segments.is_empty() || !variants.is_empty() {
                    return Err(Error::Invalid);
                }
                unsigned(value)?;
                sequence = true;
            } else if line == "#EXT-X-PLAYLIST-TYPE:VOD" {
                if playlist_type || !variants.is_empty() {
                    return Err(Error::Invalid);
                }
                playlist_type = true;
            } else if line == "#EXT-X-ENDLIST" {
                if segments.is_empty() {
                    return Err(Error::Invalid);
                }
                ended = true;
            } else {
                return Err(Error::Unsupported);
            }
            continue;
        }
        if ended {
            return Err(Error::Invalid);
        }
        let source = MediaUrl::checked(base.0.join(line).map_err(|_| Error::UnsafeUrl)?)?;
        if base.0.scheme() == "https" && source.0.scheme() != "https" {
            return Err(Error::UnsafeUrl);
        }
        if let Some((bandwidth, resolution, codecs)) = pending_variant.take() {
            if variants.len() == MAX_ENTRIES {
                return Err(Error::Limit);
            }
            variants.push(Variant {
                source,
                bandwidth,
                resolution,
                codecs,
            });
        } else if let Some(duration_seconds) = pending_duration.take() {
            if segments.len() == MAX_ENTRIES {
                return Err(Error::Limit);
            }
            segments.push(Segment {
                source,
                duration_seconds,
            });
        } else {
            return Err(Error::Invalid);
        }
    }
    if pending_variant.is_some() || pending_duration.is_some() {
        return Err(Error::Invalid);
    }
    if !variants.is_empty() {
        Ok(Manifest::Master { variants })
    } else if !segments.is_empty() && ended {
        Ok(Manifest::Vod { segments })
    } else if !segments.is_empty() {
        Err(Error::Unsupported) // Live/event playlists need a different lifecycle.
    } else {
        Err(Error::Invalid)
    }
}

fn unsigned(value: &str) -> Result<u64, Error> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Invalid);
    }
    value.parse().map_err(|_| Error::Invalid)
}

fn positive(value: &str) -> Result<u64, Error> {
    let value = unsigned(value)?;
    if value == 0 {
        Err(Error::Invalid)
    } else {
        Ok(value)
    }
}

type VariantAttributes = (u64, Option<(u32, u32)>, Option<String>);

fn parse_variant(mut text: &str) -> Result<VariantAttributes, Error> {
    let mut bandwidth = None;
    let mut resolution = None;
    let mut codecs = None;
    while !text.is_empty() {
        let (key, rest) = text.split_once('=').ok_or(Error::Invalid)?;
        let quoted = rest.starts_with('"');
        let (value, tail) = if let Some(rest) = rest.strip_prefix('"') {
            let (value, tail) = rest.split_once('"').ok_or(Error::Invalid)?;
            (value, tail)
        } else {
            rest.find(',')
                .map_or((rest, ""), |i| (&rest[..i], &rest[i..]))
        };
        match key {
            "BANDWIDTH" if bandwidth.is_none() && !quoted => bandwidth = Some(positive(value)?),
            "RESOLUTION" if resolution.is_none() && !quoted => {
                let (width, height) = value.split_once('x').ok_or(Error::Invalid)?;
                resolution = Some((
                    u32::try_from(positive(width)?).map_err(|_| Error::Invalid)?,
                    u32::try_from(positive(height)?).map_err(|_| Error::Invalid)?,
                ));
            }
            "CODECS" if codecs.is_none() && quoted && !value.is_empty() => {
                codecs = Some(value.to_owned());
            }
            "BANDWIDTH" | "RESOLUTION" | "CODECS" => return Err(Error::Invalid),
            _ => return Err(Error::Unsupported),
        }
        if tail.is_empty() {
            break;
        }
        text = tail
            .strip_prefix(',')
            .filter(|v| !v.is_empty())
            .ok_or(Error::Invalid)?;
    }
    Ok((bandwidth.ok_or(Error::Invalid)?, resolution, codecs))
}
