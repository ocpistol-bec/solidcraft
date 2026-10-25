//! Stream-name encodings: segment stream ids (spec 7.2) and property-set names (spec 8.2).

use crate::guid::Guid;

const SEGMENT_ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz123456";
const PROPSET_ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz012345";
/// The standard property sets of [MS-OLEPS] and the FMTID of each one's first section.
const FIXED_NAMES: [(&str, &str); 2] = [
    ("SummaryInformation", "f29f85e0-4ff9-1068-ab91-08002b27b3d9"),
    (
        "DocumentSummaryInformation",
        "d5cdd502-2e9c-101b-9397-08002b2cf9ae",
    ),
];

/// `GetName(seg_id)`: the 26-character id shared by a segment's `M` and `B` streams.
///
/// Each u32 word of the GUID gives six characters (bits 25–29 first), then two characters
/// carry the top two bits of the four words.
///
/// ```
/// let id = corliss::Guid::parse("81b732f4-11d1-a3ce-6000-0c8a806bceb0").unwrap();
/// assert_eq!(corliss::naming::segment_stream_id(&id), "a2omxur33eorfayadaym313akk");
/// ```
pub fn segment_stream_id(seg_id: &Guid) -> String {
    let words = seg_id.words();
    let mut out = String::with_capacity(26);
    for word in words {
        for k in (0..6).rev() {
            out.push(segment_char(word >> (5 * k)));
        }
    }
    out.push(segment_char((top2(words[0]) << 2) | top2(words[1])));
    out.push(segment_char((top2(words[2]) << 2) | top2(words[3])));
    out
}

/// `SetName`: the GUID a 26-character name encodes (the inverse of [`segment_stream_id`]);
/// `None` when `name` is not 26 characters of the name alphabet. Project files store their
/// GUIDs this way (spec 19.3, Name26).
///
/// ```
/// let id = corliss::naming::guid_from_name("a2omxur33eorfayadaym313akk").unwrap();
/// assert_eq!(id.to_string(), "81b732f4-11d1-a3ce-6000-0c8a806bceb0");
/// assert_eq!(corliss::naming::guid_from_name("aaaaaaaaaaaaaaaaaaaaaaaaaa"), Some(corliss::Guid::default()));
/// ```
pub fn guid_from_name(name: &str) -> Option<Guid> {
    let values: Vec<u32> = name.bytes().map(segment_value).collect::<Option<_>>()?;
    if values.len() != 26 {
        return None;
    }
    let top = [
        values[24] >> 2,
        values[24] & 3,
        values[25] >> 2,
        values[25] & 3,
    ];
    let mut bytes = [0u8; 16];
    for (k, chunk) in values[..24].chunks(6).enumerate() {
        let low = chunk.iter().fold(0u32, |w, v| (w << 5) | v);
        bytes[4 * k..4 * k + 4].copy_from_slice(&(low | top[k] << 30).to_le_bytes());
    }
    Some(Guid::from_bytes(bytes))
}

fn segment_value(c: u8) -> Option<u32> {
    SEGMENT_ALPHABET
        .iter()
        .position(|a| *a == c)
        .map(|p| p as u32)
}

fn top2(word: u32) -> u32 {
    word >> 30
}

fn segment_char(value: u32) -> char {
    char::from(SEGMENT_ALPHABET[(value & 31) as usize])
}

/// Decodes a property-set stream name (`\x05` + 26 characters) to its FMTID; `None` for other names.
///
/// Case is ignored, because Inventor writes some letters in upper case (OI-011).
///
/// ```
/// let fmtid = corliss::Guid::parse("32853f0f-3444-11d1-9e93-0060b03c1ca6").unwrap();
/// let name = corliss::naming::property_set_stream_name(&fmtid);
/// assert_eq!(corliss::naming::property_set_fmtid(&name), Some(fmtid));
/// ```
pub fn property_set_fmtid(name: &str) -> Option<Guid> {
    let body = name.strip_prefix('\u{5}')?;
    // Microsoft's own sets use fixed names; "DocumentSummaryInformation" is 26 characters long
    // and would otherwise decode as an encoded FMTID.
    if let Some((_, fmtid)) = FIXED_NAMES.iter().find(|(fixed, _)| *fixed == body) {
        return Guid::parse(fmtid);
    }
    let mut bits: u64 = 0;
    let mut nbits = 0;
    let mut out = Vec::with_capacity(16);
    for ch in body.chars() {
        let value = PROPSET_ALPHABET
            .iter()
            .position(|c| char::from(*c) == ch.to_ascii_lowercase())?;
        bits |= (value as u64) << nbits;
        nbits += 5;
        while nbits >= 8 && out.len() < 16 {
            out.push((bits & 0xFF) as u8);
            bits >>= 8;
            nbits -= 8;
        }
    }
    let bytes: [u8; 16] = out.try_into().ok()?;
    Some(Guid::from_bytes(bytes))
}

/// Encodes an FMTID as a property-set stream name in lower case (spec 8.2).
pub fn property_set_stream_name(fmtid: &Guid) -> String {
    let mut out = String::from('\u{5}');
    let mut bits: u64 = 0;
    let mut nbits = 0;
    let mut chars = 0;
    for byte in fmtid.as_bytes() {
        bits |= u64::from(*byte) << nbits;
        nbits += 8;
        while nbits >= 5 {
            out.push(char::from(PROPSET_ALPHABET[(bits & 31) as usize]));
            bits >>= 5;
            nbits -= 5;
            chars += 1;
        }
    }
    if chars < 26 {
        out.push(char::from(PROPSET_ALPHABET[(bits & 31) as usize]));
    }
    out
}
