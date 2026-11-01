use crate::{Error, MAX_ENTRIES, MAX_LINE_BYTES, MAX_MANIFEST_BYTES, MediaUrl, positive, resolve};
use quick_xml::{Reader, events::Event};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashSegment {
    pub source: MediaUrl,
    pub number: u64,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashTrack {
    /// Adaptation-set ID and representation ID are separate identities.
    pub adaptation_id: Option<String>,
    pub id: String,
    pub mime_type: String,
    pub codecs: Option<String>,
    pub language: Option<String>,
    pub bandwidth: u64,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub initialization: Option<MediaUrl>,
    pub timescale: u64,
    pub segment_duration: u64,
    pub segments: Vec<DashSegment>,
}

impl std::fmt::Debug for DashTrack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Manifest-supplied identity/codec/language strings may themselves contain secrets.
        f.debug_struct("DashTrack")
            .field("bandwidth", &self.bandwidth)
            .field("initialization", &self.initialization)
            .field("segments", &self.segments)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashPlan {
    pub manifest: MediaUrl,
    pub period_id: Option<String>,
    pub duration_milliseconds: Option<u64>,
    /// All representations remain choices; inspection never selects one.
    pub tracks: Vec<DashTrack>,
}

// Conservative ISO 8601 time-only duration, with at most millisecond precision.
fn duration(value: &str) -> Result<u64, Error> {
    let mut rest = value.strip_prefix("PT").ok_or(Error::Unsupported)?;
    let mut total = 0u64;
    let mut any = false;
    for (unit, multiplier) in [('H', 3_600_000u64), ('M', 60_000), ('S', 1000)] {
        if let Some((number, tail)) = rest.split_once(unit) {
            let amount = if unit == 'S' {
                if let Some((whole, fraction)) = number.split_once('.') {
                    if fraction.is_empty()
                        || fraction.len() > 3
                        || !fraction.bytes().all(|b| b.is_ascii_digit())
                    {
                        return Err(Error::Unsupported);
                    }
                    let millis = crate::unsigned(fraction)?
                        .checked_mul(10u64.pow(3 - fraction.len() as u32))
                        .ok_or(Error::Invalid)?;
                    crate::unsigned(whole)?
                        .checked_mul(1000)
                        .and_then(|n| n.checked_add(millis))
                        .ok_or(Error::Invalid)?
                } else {
                    crate::unsigned(number)?
                        .checked_mul(multiplier)
                        .ok_or(Error::Invalid)?
                }
            } else {
                crate::unsigned(number)?
                    .checked_mul(multiplier)
                    .ok_or(Error::Invalid)?
            };
            total = total.checked_add(amount).ok_or(Error::Invalid)?;
            rest = tail;
            any = true;
        }
    }
    if !any || !rest.is_empty() {
        return Err(Error::Unsupported);
    }
    Ok(total)
}

impl std::fmt::Debug for DashPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DashPlan")
            .field("manifest", &self.manifest)
            .field("tracks", &self.tracks)
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct Node {
    name: String,
    attrs: BTreeMap<String, String>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn check(&self, attrs: &[&str], children: &[&str]) -> Result<(), Error> {
        if self.attrs.keys().any(|k| !attrs.contains(&k.as_str()))
            || self
                .children
                .iter()
                .any(|c| !children.contains(&c.name.as_str()))
            || (!self.text.trim().is_empty() && self.name != "BaseURL")
        {
            return Err(Error::Unsupported);
        }
        Ok(())
    }
    fn get(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(String::as_str)
    }
    fn one(&self, name: &str) -> Result<Option<&Node>, Error> {
        let mut nodes = self.children.iter().filter(|n| n.name == name);
        let node = nodes.next();
        if nodes.next().is_some() {
            return Err(Error::Unsupported);
        }
        Ok(node)
    }
    fn base(&self, parent: &MediaUrl) -> Result<MediaUrl, Error> {
        match self.one("BaseURL")? {
            Some(node) => {
                node.check(&[], &[])?;
                if node.text.trim().is_empty() {
                    return Err(Error::Invalid);
                }
                resolve(parent, node.text.trim())
            }
            None => Ok(parent.clone()),
        }
    }
}

fn xml(body: &[u8]) -> Result<Node, Error> {
    let text = std::str::from_utf8(body).map_err(|_| Error::Invalid)?;
    let mut reader = Reader::from_str(text);
    let mut stack = vec![Node::default()];
    let mut count = 0;
    loop {
        let event = reader.read_event().map_err(|_| Error::Invalid)?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                count += 1;
                if count > MAX_ENTRIES * 4 || stack.len() > 12 {
                    return Err(Error::Limit);
                }
                let name = e.name().as_ref().to_owned();
                let mut node = Node {
                    name,
                    ..Node::default()
                };
                for attr in e.attributes() {
                    let attr = attr.map_err(|_| Error::Invalid)?;
                    let key = attr.key.as_ref().to_owned();
                    let value = attr
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|_| Error::Invalid)?
                        .into_owned();
                    if value.len() > MAX_LINE_BYTES || value.chars().any(char::is_control) {
                        return Err(Error::Limit);
                    }
                    if node.attrs.insert(key, value).is_some() {
                        return Err(Error::Invalid);
                    }
                }
                if empty {
                    stack.last_mut().ok_or(Error::Invalid)?.children.push(node);
                } else {
                    stack.push(node);
                }
            }
            Event::End(_) => {
                if stack.len() <= 1 {
                    return Err(Error::Invalid);
                }
                let node = stack.pop().ok_or(Error::Invalid)?;
                stack.last_mut().ok_or(Error::Invalid)?.children.push(node);
            }
            Event::Text(e) => {
                let value = e.xml10_content();
                let node = stack.last_mut().ok_or(Error::Invalid)?;
                if node.text.len() + value.len() > MAX_LINE_BYTES {
                    return Err(Error::Limit);
                }
                node.text.push_str(&value);
            }
            Event::GeneralRef(e) => {
                let value = match e.as_ref() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => return Err(Error::Unsupported),
                };
                let node = stack.last_mut().ok_or(Error::Invalid)?;
                if node.text.len() + value.len() > MAX_LINE_BYTES {
                    return Err(Error::Limit);
                }
                node.text.push_str(value);
            }
            Event::Decl(e) => {
                if e.xml_version().map_err(|_| Error::Invalid)?
                    != quick_xml::XmlVersion::Explicit1_0
                {
                    return Err(Error::Unsupported);
                }
            }
            Event::Comment(_) => {}
            Event::Eof => break,
            // DTD, entities, processing instructions and CDATA are not needed.
            _ => return Err(Error::Unsupported),
        }
    }
    if stack.len() != 1 {
        return Err(Error::Invalid);
    }
    let mut root = stack.pop().ok_or(Error::Invalid)?;
    if root.children.len() != 1 || !root.text.trim().is_empty() {
        return Err(Error::Invalid);
    }
    Ok(root.children.remove(0))
}

/// Strict static, single-period DASH with explicit SegmentList addressing.
/// No networking, DRM, templates, timelines, ranges, xlinks, or implicit choices.
pub fn inspect_dash(base: &str, body: &[u8]) -> Result<DashPlan, Error> {
    if body.len() > MAX_MANIFEST_BYTES {
        return Err(Error::Limit);
    }
    let manifest = MediaUrl::parse(base)?;
    let root = xml(body)?;
    if root.name != "MPD" {
        return Err(Error::Invalid);
    }
    root.check(
        &[
            "xmlns",
            "type",
            "mediaPresentationDuration",
            "minBufferTime",
            "profiles",
        ],
        &["BaseURL", "Period"],
    )?;
    if root
        .get("xmlns")
        .is_some_and(|v| v != "urn:mpeg:dash:schema:mpd:2011")
        || root.get("type") != Some("static")
    {
        return Err(Error::Unsupported);
    }
    let base = root.base(&manifest)?;
    let period = root.one("Period")?.ok_or(Error::Invalid)?;
    period.check(&["id", "duration", "start"], &["BaseURL", "AdaptationSet"])?;
    if period.get("start").is_some_and(|v| v != "PT0S") {
        return Err(Error::Unsupported);
    }
    let base = period.base(&base)?;
    let root_duration = root
        .get("mediaPresentationDuration")
        .map(duration)
        .transpose()?;
    let period_duration = period.get("duration").map(duration).transpose()?;
    if root_duration
        .zip(period_duration)
        .is_some_and(|(a, b)| a != b)
    {
        return Err(Error::Unsupported);
    }
    let duration_milliseconds = period_duration.or(root_duration);
    if duration_milliseconds == Some(0) {
        return Err(Error::Invalid);
    }
    if let Some(value) = root.get("minBufferTime") {
        duration(value)?;
    }
    let mut tracks = Vec::new();
    let mut total = 0;
    for adaptation in period.children.iter().filter(|n| n.name == "AdaptationSet") {
        adaptation.check(
            &[
                "id",
                "mimeType",
                "codecs",
                "lang",
                "contentType",
                "segmentAlignment",
            ],
            &["BaseURL", "Representation", "SegmentList"],
        )?;
        let adaptation_base = adaptation.base(&base)?;
        let inherited_list = adaptation.one("SegmentList")?;
        let before = tracks.len();
        for rep in adaptation
            .children
            .iter()
            .filter(|n| n.name == "Representation")
        {
            rep.check(
                &["id", "bandwidth", "mimeType", "codecs", "width", "height"],
                &["BaseURL", "SegmentList"],
            )?;
            let rep_base = rep.base(&adaptation_base)?;
            let id = rep
                .get("id")
                .filter(|v| !v.is_empty())
                .ok_or(Error::Invalid)?
                .to_owned();
            if tracks.iter().any(|t: &DashTrack| {
                t.id == id && t.adaptation_id.as_deref() == adaptation.get("id")
            }) {
                return Err(Error::Invalid);
            }
            let mime = rep
                .get("mimeType")
                .or(adaptation.get("mimeType"))
                .ok_or(Error::Unsupported)?;
            if !matches!(
                mime,
                "video/mp4" | "audio/mp4" | "video/mp2t" | "text/vtt" | "application/x-subrip"
            ) {
                return Err(Error::Unsupported);
            }
            let own_list = rep.one("SegmentList")?;
            if own_list.is_some() && inherited_list.is_some() {
                return Err(Error::Unsupported);
            }
            let list = own_list.or(inherited_list).ok_or(Error::Unsupported)?;
            list.check(
                &["timescale", "duration", "startNumber"],
                &["Initialization", "SegmentURL"],
            )?;
            let timescale = positive(list.get("timescale").unwrap_or("1"))?;
            let segment_duration = positive(list.get("duration").ok_or(Error::Unsupported)?)?;
            let start = positive(list.get("startNumber").unwrap_or("1"))?;
            let initialization = list
                .one("Initialization")?
                .map(|n| {
                    n.check(&["sourceURL"], &[])?;
                    resolve(
                        &rep_base,
                        n.get("sourceURL")
                            .filter(|v| !v.is_empty())
                            .ok_or(Error::Unsupported)?,
                    )
                })
                .transpose()?;
            let mut segments = Vec::new();
            for n in list.children.iter().filter(|n| n.name == "SegmentURL") {
                n.check(&["media"], &[])?;
                total += 1;
                if total > MAX_ENTRIES {
                    return Err(Error::Limit);
                }
                segments.push(DashSegment {
                    source: resolve(
                        &rep_base,
                        n.get("media")
                            .filter(|v| !v.is_empty())
                            .ok_or(Error::Invalid)?,
                    )?,
                    number: start
                        .checked_add(segments.len() as u64)
                        .ok_or(Error::Invalid)?,
                });
            }
            if segments.is_empty() {
                return Err(Error::Invalid);
            }
            if let Some(ms) = duration_milliseconds {
                let target = ms as u128 * timescale as u128;
                let unit = segment_duration as u128 * 1000;
                if target <= (segments.len() - 1) as u128 * unit
                    || target > segments.len() as u128 * unit
                {
                    return Err(Error::Unsupported);
                }
            }
            tracks.push(DashTrack {
                adaptation_id: adaptation.get("id").map(str::to_owned),
                id,
                mime_type: mime.to_owned(),
                codecs: rep
                    .get("codecs")
                    .or(adaptation.get("codecs"))
                    .map(str::to_owned),
                language: adaptation.get("lang").map(str::to_owned),
                bandwidth: positive(rep.get("bandwidth").ok_or(Error::Invalid)?)?,
                width: rep.get("width").map(positive).transpose()?,
                height: rep.get("height").map(positive).transpose()?,
                initialization,
                timescale,
                segment_duration,
                segments,
            });
        }
        if tracks.len() == before {
            return Err(Error::Unsupported);
        }
    }
    if tracks.is_empty() {
        return Err(Error::Invalid);
    }
    Ok(DashPlan {
        manifest,
        period_id: period.get("id").map(str::to_owned),
        duration_milliseconds,
        tracks,
    })
}
