//! Writing the tags beyond the nine fields [`TagEdit`] names, addressed by the keys
//! [`TagEntry`] uses: lofty's common name (`Composer`), or the format's own key behind
//! its prefix (`id3:TXXX:MY FIELD`, `vorbis:MYFIELD`, `mp4:----:com.apple.iTunes:MY
//! FIELD`).
//!
//! What a key means depends on the file: a common name becomes an ID3v2 frame in an
//! MP3, a Vorbis comment field in a FLAC file and an atom in an M4A file. So every key
//! is resolved per file to
//! the key the scan will list it under, by writing a sample value into an empty tag
//! of the file's kind and reading it back. That makes "what was written" and "what
//! reads back" the same thing, which the checks after writing rely on.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use lofty::TextEncoding;
use lofty::id3::v2::{
    CommentFrame, ExtendedTextFrame, ExtendedUrlFrame, Frame, FrameId, Id3v2Tag,
    TextInformationFrame, UnsynchronizedTextFrame, UrlLinkFrame,
};
use lofty::mp4::{Atom, AtomData, AtomIdent, Ilst};
use lofty::ogg::tag::VorbisComments;
use lofty::tag::items::UNKNOWN_LANGUAGE;
use lofty::tag::{ItemKey, ItemValue, MergeTag, SplitTag, TagItem, TagType};

use crate::edit::{FieldEdit, TagEdit};
use crate::tags::{self, Format, TagEntry};

/// The keys the nine fields of [`TagEdit`] write. An edit changes them through those
/// fields, so each value has one way to be written and one check after writing.
const SHOWN: [ItemKey; 9] = [
    ItemKey::TrackTitle,
    ItemKey::TrackArtist,
    ItemKey::AlbumTitle,
    ItemKey::AlbumArtist,
    ItemKey::TrackNumber,
    ItemKey::DiscNumber,
    ItemKey::RecordingDate,
    ItemKey::Year,
    ItemKey::Genre,
];

/// The value written to resolve a key when the edit gives none (it clears the tag).
const SAMPLE: &str = "1";

/// ID3v2 frames lofty reads as timestamps. A text frame with one of these IDs is not
/// what lofty writes back, so they are written through their common names only.
const TIMESTAMP_FRAMES: [&str; 5] = ["TDEN", "TDOR", "TDRC", "TDRL", "TDTG"];

/// The ID3v2 frames that hold a number and its total as `number/total`, with the keys
/// lofty lists the two under. lofty builds such a frame only from the keys it is
/// given, so a total given alone would replace the number with 0, and it writes
/// `MVIN`'s two as two values instead of a pair. VCMP writes these frames itself.
const NUMBER_PAIRS: [(&str, ItemKey, ItemKey); 3] = [
    ("TRCK", ItemKey::TrackNumber, ItemKey::TrackTotal),
    ("TPOS", ItemKey::DiscNumber, ItemKey::DiscTotal),
    ("MVIN", ItemKey::MovementNumber, ItemKey::MovementTotal),
];

/// A parsed tag key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    /// lofty's name, the same in every format.
    Common(ItemKey),
    /// An ID3v2 frame, with the description of a frame that has one.
    Id3 {
        id: String,
        description: Option<String>,
    },
    /// A Vorbis comment field, in capitals.
    Vorbis(String),
    /// An MP4 atom: four characters, or a freeform atom's mean and name.
    Mp4(AtomIdent<'static>),
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Common(item) => f.write_str(&tags::generic_key(*item)),
            Self::Id3 {
                id,
                description: Some(description),
            } => write!(f, "id3:{id}:{description}"),
            Self::Id3 { id, .. } => write!(f, "id3:{id}"),
            Self::Vorbis(name) => write!(f, "vorbis:{name}"),
            Self::Mp4(ident) => f.write_str(&tags::mp4_key(ident)),
        }
    }
}

/// An atom's name after `mp4:`: `----:<mean>:<name>` for a freeform atom, otherwise
/// four Latin-1 characters without control characters (many start with the
/// copyright sign).
fn parse_atom(name: &str) -> Option<AtomIdent<'static>> {
    if let Some(freeform) = name.strip_prefix("----:") {
        let (mean, name) = freeform.split_once(':')?;
        return (!mean.is_empty() && !name.is_empty()).then(|| AtomIdent::Freeform {
            mean: Cow::Owned(mean.to_owned()),
            name: Cow::Owned(name.to_owned()),
        });
    }
    let bytes: Vec<u8> = name
        .chars()
        .map(|c| {
            u8::try_from(c)
                .ok()
                .filter(|byte| !c.is_control() && *byte != 0)
        })
        .collect::<Option<_>>()?;
    let fourcc: [u8; 4] = bytes.try_into().ok()?;
    (fourcc != *b"----").then_some(AtomIdent::Fourcc(fourcc))
}

impl Key {
    pub(crate) fn parse(key: &str) -> Option<Self> {
        if let Some(rest) = key.strip_prefix("id3:") {
            let (id, description) = match rest.split_once(':') {
                Some((id, description)) => (id, Some(description.to_owned())),
                None => (rest, None),
            };
            let valid = id.len() == 4
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit());
            return valid.then(|| Self::Id3 {
                id: id.to_owned(),
                description,
            });
        }
        if let Some(name) = key.strip_prefix("vorbis:") {
            // The Vorbis comment rule: printable ASCII up to '}', without '='.
            let valid = !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| (0x20..=0x7d).contains(&byte) && byte != b'=');
            return valid.then(|| Self::Vorbis(name.to_ascii_uppercase()));
        }
        if let Some(name) = key.strip_prefix("mp4:") {
            return parse_atom(name).map(Self::Mp4);
        }
        ItemKey::VARIANTS
            .iter()
            .find(|item| tags::generic_key(**item) == key)
            .map(|item| Self::Common(*item))
    }

    /// The format whose tag a native key belongs to; None for a common name.
    fn own_format(&self) -> Option<Format> {
        match self {
            Self::Common(_) => None,
            Self::Id3 { .. } => Some(Format::Mp3),
            Self::Vorbis(_) => Some(Format::Flac),
            Self::Mp4(_) => Some(Format::Aac),
        }
    }
}

/// Why an edit of the other tags can't be written to any file.
pub(crate) enum KeyProblem {
    Unknown,
    Shown,
    /// A native key whose frame VCMP can't build.
    Unwritable,
    /// A value lofty doesn't keep under a common name in any format (a rating not in
    /// its `email|stars|count` form).
    BadValue,
}

/// Checks a key and its values the way [`TagEdit::validate`] needs: that the key
/// parses, that it isn't one of the nine fields under another name, and that every
/// value reads back after being written in at least one kind of tag. Whether the
/// file's own kind holds it is known only per file.
pub(crate) fn check_key(key: &str, values: &[String]) -> Result<(), KeyProblem> {
    let parsed = Key::parse(key).ok_or(KeyProblem::Unknown)?;
    if matches!(parsed, Key::Common(item) if SHOWN.contains(&item)) {
        return Err(KeyProblem::Shown);
    }
    // A native key may name one of the nine fields (`id3:TIT2`).
    if let Some(format) = parsed.own_format()
        && canonical(&parsed, format, SAMPLE).is_some_and(|name| is_shown(&name))
    {
        return Err(KeyProblem::Shown);
    }
    let formats = match parsed.own_format() {
        Some(format) => vec![format],
        None => vec![Format::Mp3, Format::Flac, Format::Aac],
    };
    for value in values {
        let kept = formats.iter().any(|&format| {
            probe(&parsed, format, value)
                .is_some_and(|entries| entries.iter().any(|entry| entry.value == *value))
        });
        if !kept {
            // When a kind of tag has a field for the key, the value is at fault;
            // when none has, the key is.
            return Err(match parsed {
                Key::Common(item)
                    if [TagType::Id3v2, TagType::VorbisComments, TagType::Mp4Ilst]
                        .into_iter()
                        .any(|tag_type| item.map_key(tag_type).is_some()) =>
                {
                    KeyProblem::BadValue
                }
                _ => KeyProblem::Unwritable,
            });
        }
    }
    Ok(())
}

/// Whether the key is one lofty keeps as a number (a total, a movement number).
pub(crate) fn takes_number(key: &str) -> bool {
    matches!(Key::parse(key), Some(Key::Common(item)) if item.is_numeric())
}

fn is_shown(name: &str) -> bool {
    matches!(Key::parse(name), Some(Key::Common(item)) if SHOWN.contains(&item))
}

/// One edit of [`TagEdit::other`], resolved for a file.
#[derive(Debug)]
pub(crate) struct Resolved {
    key: Key,
    /// The key the scan lists the values under.
    name: String,
    /// None clears the tag.
    values: Option<Vec<String>>,
}

/// The keys `edit` may change in a file that holds the `existing` keys: those of the
/// nine fields it edits (whether the file has them yet or not), those its other edits
/// resolve to, and the ones the nine fields' writes remove along the way (a user text
/// frame holding an album artist, ID3v2.3 date parts, a total cleared with its number).
pub(crate) fn touched_keys<'a>(
    edit: &TagEdit,
    resolved: &[Resolved],
    existing: impl Iterator<Item = &'a str>,
) -> BTreeSet<String> {
    let mut keys: BTreeSet<String> = resolved.iter().map(|edit| edit.name.clone()).collect();
    for (edited, item) in [
        (!edit.title.is_keep(), ItemKey::TrackTitle),
        (!edit.artist.is_keep(), ItemKey::TrackArtist),
        (!edit.album.is_keep(), ItemKey::AlbumTitle),
        (!edit.album_artist.is_keep(), ItemKey::AlbumArtist),
        (!edit.track_number.is_keep(), ItemKey::TrackNumber),
        (!edit.disc_number.is_keep(), ItemKey::DiscNumber),
        (!edit.year.is_keep(), ItemKey::RecordingDate),
        (!edit.genre.is_keep(), ItemKey::Genre),
    ] {
        if edited {
            keys.insert(tags::generic_key(item));
        }
    }
    keys.extend(
        existing
            .filter(|key| touched_by_fields(edit, key))
            .map(str::to_owned),
    );
    keys
}

/// Whether any of the edits is to a key of the nine fields, as an undo's are.
pub(crate) fn names_shown(resolved: &[Resolved]) -> bool {
    resolved.iter().any(|edit| is_shown(&edit.name))
}

/// The values the edits give `item`: None when they leave it alone, an empty list
/// when they clear it.
pub(crate) fn values_of(resolved: &[Resolved], item: ItemKey) -> Option<&[String]> {
    let name = tags::generic_key(item);
    resolved
        .iter()
        .find(|edit| edit.name == name)
        .map(|edit| edit.values.as_deref().unwrap_or_default())
}

/// Resolves the edits for a file of `format`. Fails with the key that the file's tag
/// can't hold, that names the same tag as another key of the edit, or that is one of
/// the nine fields' keys while `raw` is off.
pub(crate) fn resolve(
    other: &BTreeMap<String, FieldEdit<Vec<String>>>,
    format: Format,
    raw: bool,
) -> Result<Vec<Resolved>, String> {
    let mut resolved: Vec<Resolved> = Vec::new();
    for (key, edit) in other {
        let values = match edit {
            FieldEdit::Keep => continue,
            FieldEdit::Set(values) if !values.is_empty() => Some(values.clone()),
            FieldEdit::Set(_) | FieldEdit::Clear => None,
        };
        let parsed = Key::parse(key).ok_or_else(|| key.clone())?;
        if parsed
            .own_format()
            .is_some_and(|own| !same_tag(own, format))
        {
            return Err(key.clone());
        }
        // The value itself resolves the key: some tags take only values of a form
        // (a rating is `email|stars|count`), and a sample of another form is dropped.
        let sample = values
            .as_ref()
            .and_then(|values| values.first())
            .map_or(SAMPLE, String::as_str);
        let name = match canonical(&parsed, format, sample) {
            Some(name) => name,
            // Clearing needs no value: a frame or field the writer can't make (a list
            // of performers) can still be removed under its listed key.
            None if values.is_none() => parsed.to_string(),
            None => return Err(key.clone()),
        };
        if (!raw && is_shown(&name)) || resolved.iter().any(|other| other.name == name) {
            return Err(key.clone());
        }
        let key = Key::parse(&name).ok_or_else(|| key.clone())?;
        resolved.push(Resolved { key, name, values });
    }
    Ok(resolved)
}

/// The kinds of tag VCMP writes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TagKind {
    Id3v2,
    Vorbis,
    Mp4,
}

fn tag_kind(format: Format) -> TagKind {
    match format {
        Format::Mp3 | Format::Wav | Format::Aiff => TagKind::Id3v2,
        Format::Flac | Format::OggVorbis => TagKind::Vorbis,
        Format::Aac | Format::Alac => TagKind::Mp4,
    }
}

/// Whether files of both formats carry the same kind of tag.
fn same_tag(a: Format, b: Format) -> bool {
    tag_kind(a) == tag_kind(b)
}

/// Writes `value` under `key` into an empty tag of the kind a file of `format` has,
/// and lists it the way a scan does. None when that kind of tag can't take it.
fn probe(key: &Key, format: Format, value: &str) -> Option<Vec<TagEntry>> {
    let sample = [value.to_owned()];
    let edit = [(key, Some(&sample[..]))];
    Some(match tag_kind(format) {
        TagKind::Id3v2 => {
            let mut tag = Id3v2Tag::new();
            apply_id3v2(&mut tag, &edit).ok()?;
            tags::id3v2_entries(&tag)
        }
        TagKind::Vorbis => {
            let mut tag = VorbisComments::default();
            apply_vorbis(&mut tag, &edit).ok()?;
            tags::vorbis_entries(&tag)
        }
        TagKind::Mp4 => {
            let mut tag = Ilst::new();
            apply_mp4(&mut tag, &edit).ok()?;
            tags::mp4_entries(&tag)
        }
    })
}

/// The key the scan lists `value` written under `key` as, in a file of `format`; None
/// when that file's tag can't hold it.
fn canonical(key: &Key, format: Format, value: &str) -> Option<String> {
    let entries = probe(key, format, value)?;
    let wanted = key.to_string();
    if entries.iter().any(|entry| entry.key == wanted) {
        return Some(wanted);
    }
    // A key read back under another name (`id3:TCOM` is listed as `Composer`).
    let first = &entries.first()?.key;
    entries
        .iter()
        .all(|entry| entry.key == *first)
        .then(|| first.clone())
}

fn as_edits(resolved: &[Resolved]) -> Vec<(&Key, Option<&[String]>)> {
    resolved
        .iter()
        .map(|edit| (&edit.key, edit.values.as_deref()))
        .collect()
}

/// Applies resolved edits to an ID3v2 tag.
pub(crate) fn edit_id3v2(tag: &mut Id3v2Tag, resolved: &[Resolved]) -> Result<(), String> {
    apply_id3v2(tag, &as_edits(resolved))
}

/// Applies resolved edits to Vorbis comments.
pub(crate) fn edit_vorbis(tag: &mut VorbisComments, resolved: &[Resolved]) -> Result<(), String> {
    apply_vorbis(tag, &as_edits(resolved))
}

fn apply_id3v2(tag: &mut Id3v2Tag, edits: &[(&Key, Option<&[String]>)]) -> Result<(), String> {
    let mut common = Vec::new();
    for &(key, values) in edits {
        match key {
            Key::Common(item) => common.push((*item, values)),
            Key::Id3 { id, description } => {
                let name = key.to_string();
                tag.retain(|frame| !frame_keys(frame).contains(&name));
                if let Some(values) = values {
                    tag.insert(native_frame(id, description.as_deref(), values)?);
                }
            }
            Key::Vorbis(_) => return Err(format!("{key} is a Vorbis comment field")),
            Key::Mp4(_) => return Err(format!("{key} is an MP4 atom")),
        }
    }
    for pair in NUMBER_PAIRS {
        apply_number_pair(tag, &mut common, pair)?;
    }
    if common.is_empty() {
        return Ok(());
    }
    // The frames holding an edited key go through lofty's generic form and back, so
    // a frame holding several keys (TIPL holds the producer and the engineer) keeps
    // the ones not edited. Every other frame stays exactly as it was.
    let names: Vec<String> = common
        .iter()
        .map(|(item, _)| tags::generic_key(*item))
        .collect();
    let affected = |frame: &Frame<'static>| frame_keys(frame).iter().any(|key| names.contains(key));
    let mut scratch = Id3v2Tag::new();
    for frame in tag.iter().filter(|frame| affected(frame)) {
        scratch.insert(frame.clone());
    }
    tag.retain(|frame| !affected(frame));
    let (remainder, mut generic) = scratch.split_tag();
    for &(item, values) in &common {
        generic.remove_key(item);
        for value in values.unwrap_or_default() {
            if !generic.push(TagItem::new(item, ItemValue::Text(value.clone()))) {
                return Err(format!(
                    "ID3v2 has no frame for {}",
                    tags::generic_key(item)
                ));
            }
        }
    }
    for frame in remainder.merge_tag(generic) {
        tag.insert(frame);
    }
    Ok(())
}

/// Takes the edits of one of [`NUMBER_PAIRS`] out of `common` and writes them into
/// the frame, which keeps the part not edited.
fn apply_number_pair(
    tag: &mut Id3v2Tag,
    common: &mut Vec<(ItemKey, Option<&[String]>)>,
    (id, number_key, total_key): (&'static str, ItemKey, ItemKey),
) -> Result<(), String> {
    let (mut number_edit, mut total_edit) = (None, None);
    common.retain(|&(item, values)| {
        if item == number_key {
            number_edit = Some(values);
        } else if item == total_key {
            total_edit = Some(values);
        }
        item != number_key && item != total_key
    });
    if number_edit.is_none() && total_edit.is_none() {
        return Ok(());
    }
    let frame_id = FrameId::Valid(Cow::Borrowed(id));
    let held = tag.get(&frame_id).map(frame_entries).unwrap_or_default();
    let part = |item: ItemKey, edit: Option<Option<&[String]>>| {
        let name = tags::generic_key(item);
        match edit.map(Option::unwrap_or_default) {
            None => Ok(held
                .iter()
                .find(|entry| entry.key == name)
                .map(|entry| entry.value.clone())),
            Some([]) => Ok(None),
            Some([value]) => Ok(Some(value.clone())),
            Some(_) => Err(format!("{id} holds one {name}")),
        }
    };
    let number = part(number_key, number_edit)?;
    let total = part(total_key, total_edit)?;
    let _ = tag.remove(&frame_id);
    let content = match (number, total) {
        (None, None) => return Ok(()),
        (Some(number), None) => number,
        // A total needs a number in front of it; lofty puts 0 there too. The scan
        // then reads a number nobody asked for, which the checks after writing refuse.
        (number, Some(total)) => format!("{}/{total}", number.as_deref().unwrap_or("0")),
    };
    tag.insert(Frame::Text(TextInformationFrame::new(
        frame_id,
        TextEncoding::UTF8,
        content,
    )));
    Ok(())
}

/// What the scan lists a frame's values as.
fn frame_entries(frame: &Frame<'static>) -> Vec<TagEntry> {
    let mut single = Id3v2Tag::new();
    single.insert(frame.clone());
    tags::id3v2_entries(&single)
}

/// The keys the scan lists a frame's values under.
fn frame_keys(frame: &Frame<'static>) -> Vec<String> {
    frame_entries(frame)
        .into_iter()
        .map(|entry| entry.key)
        .collect()
}

fn native_frame(
    id: &str,
    description: Option<&str>,
    values: &[String],
) -> Result<Frame<'static>, String> {
    let joined = values.join("\0");
    let description = description.unwrap_or_default().to_owned();
    let single = || match values {
        [value] => Ok(value.clone()),
        _ => Err(format!("{id} holds a single address")),
    };
    let frame_id =
        || FrameId::new(Cow::Owned(id.to_owned())).map_err(|error| format!("{id}: {error}"));
    Ok(match id {
        "TXXX" => Frame::UserText(ExtendedTextFrame::new(
            TextEncoding::UTF8,
            description,
            joined,
        )),
        "WXXX" => Frame::UserUrl(ExtendedUrlFrame::new(
            TextEncoding::UTF8,
            description,
            single()?,
        )),
        "COMM" => Frame::Comment(CommentFrame::new(
            TextEncoding::UTF8,
            UNKNOWN_LANGUAGE,
            description,
            joined,
        )),
        "USLT" => Frame::UnsynchronizedText(UnsynchronizedTextFrame::new(
            TextEncoding::UTF8,
            UNKNOWN_LANGUAGE,
            description,
            joined,
        )),
        _ if !description.is_empty() || TIMESTAMP_FRAMES.contains(&id) => {
            return Err(format!("VCMP can't write {id} frames"));
        }
        _ if id.starts_with('T') => Frame::Text(TextInformationFrame::new(
            frame_id()?,
            TextEncoding::UTF8,
            joined,
        )),
        _ if id.starts_with('W') => Frame::Url(UrlLinkFrame::new(frame_id()?, single()?)),
        _ => return Err(format!("VCMP can't write {id} frames")),
    })
}

fn apply_vorbis(
    tag: &mut VorbisComments,
    edits: &[(&Key, Option<&[String]>)],
) -> Result<(), String> {
    for &(key, values) in edits {
        let field = match key {
            Key::Common(item) => {
                let item = *item;
                remove_fields(tag, |field| {
                    ItemKey::from_key(TagType::VorbisComments, field) == Some(item)
                });
                item.map_key(TagType::VorbisComments)
                    .ok_or_else(|| format!("Vorbis comments have no field for {key}"))?
                    .to_owned()
            }
            Key::Vorbis(name) => {
                remove_fields(tag, |field| field.eq_ignore_ascii_case(name));
                name.clone()
            }
            Key::Id3 { .. } => return Err(format!("{key} is an ID3v2 frame")),
            Key::Mp4(_) => return Err(format!("{key} is an MP4 atom")),
        };
        for value in values.unwrap_or_default() {
            tag.push(field.clone(), value.clone());
        }
    }
    Ok(())
}

/// Applies resolved edits to an MP4 `ilst`.
pub(crate) fn edit_mp4(tag: &mut Ilst, resolved: &[Resolved]) -> Result<(), String> {
    apply_mp4(tag, &as_edits(resolved))
}

/// How an atom stores its values. Atoms holding numbers or flags keep doing so when
/// VCMP writes them: players read `tmpo` or `stik` only as numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AtomKind {
    Text,
    Signed,
    Unsigned,
    Flag,
}

/// The kind of the values in `atom`; None when it holds binary data, which VCMP can't
/// write.
fn atom_kind(atom: &Atom<'_>) -> Option<AtomKind> {
    let mut kind = AtomKind::Text;
    for data in atom.data() {
        kind = match data {
            AtomData::UTF8(_) | AtomData::UTF16(_) => AtomKind::Text,
            AtomData::SignedInteger(_) => AtomKind::Signed,
            AtomData::UnsignedInteger(_) => AtomKind::Unsigned,
            AtomData::Bool(_) => AtomKind::Flag,
            AtomData::Picture(_) | AtomData::Unknown { .. } => return None,
        };
    }
    Some(kind)
}

fn atom_value(kind: AtomKind, value: &str) -> Option<AtomData> {
    Some(match kind {
        AtomKind::Text => AtomData::UTF8(value.to_owned()),
        AtomKind::Signed => AtomData::SignedInteger(value.parse().ok()?),
        AtomKind::Unsigned => AtomData::UnsignedInteger(value.parse().ok()?),
        AtomKind::Flag => AtomData::Bool(match value {
            "0" => false,
            "1" => true,
            _ => return None,
        }),
    })
}

fn new_atom(
    ident: AtomIdent<'static>,
    kind: AtomKind,
    values: &[String],
    key: &str,
) -> Result<Option<Atom<'static>>, String> {
    let data = values
        .iter()
        .map(|value| atom_value(kind, value))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| match kind {
            AtomKind::Flag => format!("{key} holds 0 or 1 in this file"),
            _ => format!("{key} holds whole numbers in this file"),
        })?;
    Ok(Atom::from_collection(ident, data))
}

fn apply_mp4(tag: &mut Ilst, edits: &[(&Key, Option<&[String]>)]) -> Result<(), String> {
    let mut common = Vec::new();
    for &(key, values) in edits {
        match key {
            Key::Common(item) => common.push((*item, values)),
            Key::Mp4(ident) => {
                let name = key.to_string();
                let kind = tag.get(ident).map_or(Some(AtomKind::Text), atom_kind);
                if let Some(values) = values {
                    let kind = kind.ok_or_else(|| format!("{name} holds data VCMP can't write"))?;
                    tag.remove(ident).for_each(drop);
                    if let Some(atom) = new_atom(ident.clone(), kind, values, &name)? {
                        tag.insert(atom);
                    }
                } else {
                    tag.remove(ident).for_each(drop);
                }
            }
            Key::Id3 { .. } => return Err(format!("{key} is an ID3v2 frame")),
            Key::Vorbis(_) => return Err(format!("{key} is a Vorbis comment field")),
        }
    }
    if common.is_empty() {
        return Ok(());
    }
    // As for ID3v2, the atoms holding an edited key go through lofty's generic form and
    // back, so an atom holding two keys (`trkn` holds the track number and total)
    // keeps the one not edited. Every other atom stays exactly as it was.
    let names: Vec<String> = common
        .iter()
        .map(|(item, _)| tags::generic_key(*item))
        .collect();
    let holds_edited =
        |atom: &Atom<'static>| tags::atom_keys(atom).iter().any(|key| names.contains(key));
    // The atoms the edited keys are written to count too, whatever they list now: a
    // `trkn` without a total lists only the number, and the total goes into it.
    let targets: Vec<AtomIdent<'static>> = common
        .iter()
        .filter_map(|(item, _)| AtomIdent::try_from(item).ok().map(AtomIdent::into_owned))
        .collect();
    let affected: Vec<AtomIdent<'static>> = (&*tag)
        .into_iter()
        .filter(|atom| holds_edited(atom) || targets.contains(atom.ident()))
        .map(|atom| atom.ident().clone().into_owned())
        .collect();
    let mut scratch = Ilst::new();
    let mut kinds = Vec::new();
    for ident in &affected {
        for atom in tag.remove(ident).collect::<Vec<_>>() {
            if let Some(kind @ (AtomKind::Signed | AtomKind::Unsigned)) = atom_kind(&atom) {
                kinds.push((ident.clone(), kind));
            }
            scratch.insert(atom);
        }
    }
    let (remainder, mut generic) = scratch.split_tag();
    for &(item, values) in &common {
        generic.remove_key(item);
        for value in values.unwrap_or_default() {
            if !generic.push(TagItem::new(item, ItemValue::Text(value.clone()))) {
                return Err(format!("MP4 has no atom for {}", tags::generic_key(item)));
            }
        }
    }
    // What lofty's split leaves in an atom of an edited key (a number stored as an
    // integer) is replaced by the edit too.
    for atom in Ilst::from(remainder) {
        if !holds_edited(&atom) && !targets.contains(atom.ident()) {
            tag.insert(atom);
        }
    }
    for atom in Ilst::from(generic) {
        let kind = kinds
            .iter()
            .find(|(ident, _)| ident == atom.ident())
            .map(|(_, kind)| *kind);
        let atom = match kind {
            Some(kind) => renumbered(atom, kind)?,
            None => atom,
        };
        tag.insert(atom);
    }
    Ok(())
}

/// `atom` with its text values written as numbers of `kind`, as the atom it replaces
/// held them; lofty's generic form writes them as text.
fn renumbered(atom: Atom<'static>, kind: AtomKind) -> Result<Atom<'static>, String> {
    let ident = atom.ident().clone().into_owned();
    let key = tags::mp4_atom_key(&ident);
    let data = atom
        .into_data()
        .map(|data| match data {
            AtomData::UTF8(text) | AtomData::UTF16(text) => atom_value(kind, &text)
                .ok_or_else(|| format!("{key} holds whole numbers in this file")),
            other => Ok(other),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Atom::from_collection(ident, data).ok_or_else(|| format!("{key} has no values"))
}

/// The fields of Vorbis comments, in order.
pub(crate) fn vorbis_fields(tag: &VorbisComments) -> Vec<(String, String)> {
    tag.items()
        .map(|(field, value)| (field.to_owned(), value.to_owned()))
        .collect()
}

/// Puts the fields `tag` still has from `original` back in their original order,
/// followed by the new ones. lofty's `remove`, which its accessors use to clear a
/// field, reorders the fields it keeps and can swap two values of one field (an
/// `ORIGINALDATE` and an `ORIGINALYEAR` both listed as `OriginalReleaseDate`).
pub(crate) fn keep_vorbis_order(tag: &mut VorbisComments, original: &[(String, String)]) {
    let mut current: Vec<Option<(String, String)>> =
        vorbis_fields(tag).into_iter().map(Some).collect();
    let mut ordered = Vec::with_capacity(current.len());
    for (field, value) in original {
        let kept = current.iter_mut().find(|slot| {
            slot.as_ref()
                .is_some_and(|(f, v)| f.eq_ignore_ascii_case(field) && v == value)
        });
        if let Some(slot) = kept {
            ordered.extend(slot.take());
        }
    }
    ordered.extend(current.into_iter().flatten());
    replace_fields(tag, ordered);
}

/// Removes the fields `matches` picks, keeping the order of the rest.
fn remove_fields(tag: &mut VorbisComments, matches: impl Fn(&str) -> bool) {
    let fields = vorbis_fields(tag);
    if fields.iter().any(|(field, _)| matches(field)) {
        replace_fields(
            tag,
            fields
                .into_iter()
                .filter(|(field, _)| !matches(field))
                .collect(),
        );
    }
}

fn replace_fields(tag: &mut VorbisComments, fields: Vec<(String, String)>) {
    let mut names: Vec<String> = vorbis_fields(tag)
        .into_iter()
        .map(|(field, _)| field.to_ascii_uppercase())
        .collect();
    names.sort();
    names.dedup();
    for name in names {
        tag.remove(&name).for_each(drop);
    }
    for (field, value) in fields {
        tag.push(field, value);
    }
}

/// Values by key, in order.
fn by_key(entries: &[TagEntry]) -> BTreeMap<&str, Vec<&str>> {
    let mut map: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for entry in entries {
        map.entry(&entry.key).or_default().push(&entry.value);
    }
    map
}

/// Checks the file read back after writing. Every edited key holds exactly the values
/// asked for. Unless VCMP had to create the tag (an MP3 with only ID3v1), every key the
/// edit didn't touch holds what it held before, in the same order.
pub(crate) fn check(
    before: &[TagEntry],
    after: &[TagEntry],
    edit: &TagEdit,
    resolved: &[Resolved],
    created: bool,
) -> Result<(), String> {
    let before = by_key(before);
    let after = by_key(after);
    for edited in resolved {
        let wanted: Vec<&str> = edited.values.iter().flatten().map(String::as_str).collect();
        let found = after.get(edited.name.as_str()).cloned().unwrap_or_default();
        if found != wanted {
            return Err(format!(
                "{}: expected {wanted:?}, read {found:?}",
                edited.name
            ));
        }
    }
    if created {
        return Ok(());
    }
    for key in before.keys().chain(after.keys()) {
        if resolved.iter().any(|edited| edited.name == *key) || touched_by_fields(edit, key) {
            continue;
        }
        if before.get(key) != after.get(key) {
            return Err(format!(
                "{key} changed from {:?} to {:?}",
                before.get(key),
                after.get(key)
            ));
        }
    }
    Ok(())
}

/// Whether the nine fields of `edit` may change the values listed under `key`.
fn touched_by_fields(edit: &TagEdit, key: &str) -> bool {
    let is = |item: ItemKey| key == tags::generic_key(item);
    (!edit.title.is_keep() && is(ItemKey::TrackTitle))
        || (!edit.artist.is_keep() && is(ItemKey::TrackArtist))
        || (!edit.album.is_keep() && is(ItemKey::AlbumTitle))
        || (!edit.album_artist.is_keep()
            && (is(ItemKey::AlbumArtist)
                || key
                    .strip_prefix("id3:TXXX:")
                    .is_some_and(crate::edit::is_album_artist_key)))
        // Clearing the number drops the total stored with it.
        || (!edit.track_number.is_keep()
            && (is(ItemKey::TrackNumber)
                || (matches!(edit.track_number, FieldEdit::Clear) && is(ItemKey::TrackTotal))))
        || (!edit.disc_number.is_keep()
            && (is(ItemKey::DiscNumber)
                || (matches!(edit.disc_number, FieldEdit::Clear) && is(ItemKey::DiscTotal))))
        || (!edit.year.is_keep()
            && (is(ItemKey::RecordingDate)
                || is(ItemKey::Year)
                || matches!(key, "id3:TDRC" | "id3:TYER" | "id3:TDAT" | "id3:TIME")))
        || (!edit.genre.is_keep() && is(ItemKey::Genre))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(values: &[&str]) -> FieldEdit<Vec<String>> {
        FieldEdit::Set(values.iter().map(|value| (*value).to_owned()).collect())
    }

    #[test]
    fn mp4_keys_parse_and_print_back() {
        for key in [
            "mp4:----:com.apple.iTunes:MY FIELD",
            "mp4:----:com.example:a:b",
            "mp4:stik",
            "mp4:\u{a9}xyz",
        ] {
            let parsed = Key::parse(key).map(|parsed| parsed.to_string());
            assert_eq!(parsed.as_deref(), Some(key));
        }
        for key in [
            "mp4:",
            "mp4:abc",
            "mp4:abcde",
            "mp4:----",
            "mp4:----:mean",
            "mp4:----::name",
            "mp4:ab\u{1}c",
            "mp4:\u{100}abc",
        ] {
            assert_eq!(Key::parse(key), None, "{key}");
        }
    }

    #[test]
    fn an_mp4_key_resolves_to_the_name_it_reads_back_under() {
        // `(c)wrt` is the composer, `(c)nam` the title.
        let edit = BTreeMap::from([("mp4:\u{a9}wrt".to_owned(), set(&["Native"]))]);
        let resolved = resolve(&edit, Format::Alac, false).unwrap();
        assert_eq!(resolved[0].name, "Composer");
        assert!(matches!(
            check_key("mp4:\u{a9}nam", &[]),
            Err(KeyProblem::Shown)
        ));
        // An MP4 atom has no place in an ID3v2 tag.
        let edit = BTreeMap::from([("mp4:stik".to_owned(), set(&["1"]))]);
        assert_eq!(resolve(&edit, Format::Aiff, false).unwrap_err(), "mp4:stik");
    }

    #[test]
    fn losing_an_mp4_freeform_atom_fails_the_check() {
        let entry = |key: &str, value: &str| TagEntry {
            key: key.to_owned(),
            value: value.to_owned(),
        };
        let before = [
            entry("TrackTitle", "Old"),
            entry("mp4:----:com.example:Mood", "calm"),
        ];
        let edit = TagEdit {
            title: FieldEdit::Set("New".into()),
            ..TagEdit::default()
        };
        let kept = [
            entry("TrackTitle", "New"),
            entry("mp4:----:com.example:Mood", "calm"),
        ];
        assert_eq!(check(&before, &kept, &edit, &[], false), Ok(()));
        let lost = [entry("TrackTitle", "New")];
        let error = check(&before, &lost, &edit, &[], false).unwrap_err();
        assert!(error.contains("mp4:----:com.example:Mood"), "{error}");
    }
}
