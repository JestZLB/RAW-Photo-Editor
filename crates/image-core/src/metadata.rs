//! EXIF surgery for exports. The app bakes orientation into pixels and never
//! re-creates thumbnails, so a copied EXIF block must be adjusted or it would
//! rotate the photo twice and ship the pre-edit (often full-frame) original
//! thumbnail, which also leaks the picture the user cropped away.
//!
//! Everything here works on the raw TIFF block that `image` hands back from
//! `exif_metadata()` — the `Exif\0\0` identifier is added by the encoders.

use std::path::Path;

/// How much of the source metadata an export should carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataPolicy {
    /// Camera and shooting tags, minus the baked orientation and stale thumbnail.
    Keep,
    /// `Keep` plus removal of location data (the GPS IFD).
    Privacy,
    /// No EXIF at all.
    None,
}

const TAG_ORIENTATION: u16 = 0x0112;
const TAG_GPS_INFO: u16 = 0x8825;
const TAG_EXIF_IFD: u16 = 0x8769;
const TAG_THUMBNAIL_OFFSET: u16 = 0x0201;
const TAG_THUMBNAIL_LENGTH: u16 = 0x0202;
const TAG_MAKE: u16 = 0x010F;
const TAG_MODEL: u16 = 0x0110;
const TAG_SOFTWARE: u16 = 0x0131;
const TAG_DATETIME: u16 = 0x0132;
const TAG_ARTIST: u16 = 0x013B;
const TAG_COPYRIGHT: u16 = 0x8298;
const TAG_INTEROP_IFD: u16 = 0xA005;
const TAG_ICC_PROFILE: u16 = 0x8773;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Endianness(bool); // true = little endian ("II")

fn read_u16(bytes: &[u8], offset: usize, little: bool) -> Option<u16> {
    let pair = bytes.get(offset..offset + 2)?;
    Some(if little {
        u16::from_le_bytes([pair[0], pair[1]])
    } else {
        u16::from_be_bytes([pair[0], pair[1]])
    })
}

fn read_u32(bytes: &[u8], offset: usize, little: bool) -> Option<u32> {
    let quad = bytes.get(offset..offset + 4)?;
    Some(if little {
        u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]])
    } else {
        u32::from_be_bytes([quad[0], quad[1], quad[2], quad[3]])
    })
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16, little: bool) {
    let encoded = if little {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    };
    if let Some(slot) = bytes.get_mut(offset..offset + 2) {
        slot.copy_from_slice(&encoded);
    }
}

fn parse_header(tiff: &[u8]) -> Option<(Endianness, usize)> {
    if tiff.len() < 8 {
        return None;
    }
    let little = match &tiff[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    if read_u16(tiff, 2, little)? != 0x002A {
        return None;
    }
    let ifd0 = read_u32(tiff, 4, little)? as usize;
    if ifd0 + 2 > tiff.len() {
        return None;
    }
    Some((Endianness(little), ifd0))
}

/// Byte offset of the 12-byte entry for `tag` inside the IFD, if present.
fn find_entry(tiff: &[u8], ifd: usize, tag: u16, little: bool) -> Option<usize> {
    let count = read_u16(tiff, ifd, little)? as usize;
    if count > 1000 || ifd + 2 + count * 12 > tiff.len() {
        return None;
    }
    (0..count).find_map(|index| {
        let entry = ifd + 2 + index * 12;
        (read_u16(tiff, entry, little)? == tag).then_some(entry)
    })
}

/// Drop `tags` from an IFD by rebuilding its entry list. The block keeps its
/// length; the freed bytes after the shortened list become unreachable, which
/// stays valid TIFF because every other offset is absolute.
fn remove_entries(tiff: &mut [u8], ifd: usize, tags: &[u16]) -> usize {
    let little = matches!(&tiff[0..2], b"II");
    let original_count = match read_u16(tiff, ifd, little) {
        Some(count) => count as usize,
        None => return 0,
    };
    if original_count > 1000 || ifd + 2 + original_count * 12 + 4 > tiff.len() {
        return 0;
    }
    let mut kept = Vec::with_capacity(original_count * 12);
    for index in 0..original_count {
        let entry = ifd + 2 + index * 12;
        let tag = match read_u16(tiff, entry, little) {
            Some(tag) => tag,
            None => break,
        };
        if !tags.contains(&tag) {
            kept.extend_from_slice(&tiff[entry..entry + 12]);
        }
    }
    let removed = original_count * 12 - kept.len();
    if removed == 0 {
        return 0;
    }
    let region_start = ifd + 2;
    let next_pointer = region_start + original_count * 12;
    let next_bytes = tiff[next_pointer..next_pointer + 4].to_vec();
    tiff[region_start..next_pointer + 4].fill(0);
    tiff[region_start..region_start + kept.len()].copy_from_slice(&kept);
    let tail = region_start + kept.len();
    tiff[tail..tail + 4].copy_from_slice(&next_bytes);
    write_u16(tiff, ifd, (original_count - removed / 12) as u16, little);
    removed / 12
}

/// Rewrite the exported EXIF according to `policy`. Returns `None` when the
/// block is unusable or the policy forbids metadata.
pub fn prepare_exif(tiff: &[u8], policy: MetadataPolicy) -> Option<Vec<u8>> {
    if policy == MetadataPolicy::None {
        return None;
    }
    let (Endianness(little), ifd0) = parse_header(tiff)?;
    let mut out = tiff.to_vec();

    // Orientation is already applied to the pixels.
    if let Some(entry) = find_entry(&out, ifd0, TAG_ORIENTATION, little) {
        write_u16(&mut out, entry + 8, 1, little);
    }

    let mut tags = vec![TAG_THUMBNAIL_OFFSET, TAG_THUMBNAIL_LENGTH];
    if policy == MetadataPolicy::Privacy {
        tags.push(TAG_GPS_INFO);
    }
    remove_entries(&mut out, ifd0, &tags);

    // The thumbnail tags live in the Exif sub-IFD on real camera files.
    if let Some(entry) = find_entry(&out, ifd0, TAG_EXIF_IFD, little) {
        let offset = read_u32(&out, entry + 8, little)? as usize;
        if offset + 2 <= out.len() {
            remove_entries(
                &mut out,
                offset,
                &[TAG_THUMBNAIL_OFFSET, TAG_THUMBNAIL_LENGTH],
            );
        }
    }
    Some(out)
}

/// True when the block carries location data, so the UI can warn.
pub fn has_gps(tiff: &[u8]) -> bool {
    match parse_header(tiff) {
        Some((Endianness(little), ifd0)) => find_entry(tiff, ifd0, TAG_GPS_INFO, little).is_some(),
        None => false,
    }
}

/// True when the block has an embedded thumbnail.
pub fn has_thumbnail(tiff: &[u8]) -> bool {
    let Some((Endianness(little), ifd0)) = parse_header(tiff) else {
        return false;
    };
    if find_entry(tiff, ifd0, TAG_THUMBNAIL_OFFSET, little).is_some() {
        return true;
    }
    match find_entry(tiff, ifd0, TAG_EXIF_IFD, little) {
        Some(entry) => {
            let offset = read_u32(tiff, entry + 8, little).unwrap_or(0) as usize;
            offset + 2 <= tiff.len()
                && find_entry(tiff, offset, TAG_THUMBNAIL_OFFSET, little).is_some()
        }
        None => false,
    }
}

// ---- TIFF EXIF embedding -------------------------------------------------
//
// `image`'s TIFF encoder has no EXIF hook (the trait default rejects it), so a
// finished TIFF is rewritten here: the camera tags are appended as a fresh
// Exif IFD, IFD0 is rebuilt with the extra pointer at the end of the file, and
// the header is pointed at it. Existing pixel data and strip offsets stay put.

/// Byte size of one element of a TIFF field type.
fn type_size(typ: u16) -> Option<usize> {
    Some(match typ {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => return None,
    })
}

/// Element size used when byte-swapping a field type (RATIONAL swaps each u32).
fn swap_element(typ: u16) -> usize {
    match typ {
        3 | 8 => 2,
        4 | 9 | 11 | 5 | 10 => 4,
        12 => 8,
        _ => 1,
    }
}

fn swap_endian(value: &[u8], element: usize) -> Vec<u8> {
    let mut out = value.to_vec();
    if element > 1 {
        for chunk in out.chunks_exact_mut(element) {
            chunk.reverse();
        }
    }
    out
}

/// `(type, count, payload)` for an IFD entry, following out-of-line values.
fn entry_payload(src: &[u8], entry: usize, little: bool) -> Option<(u16, u32, &[u8])> {
    let typ = read_u16(src, entry + 2, little)?;
    let count = read_u32(src, entry + 4, little)?;
    let size = type_size(typ)? as u64 * count as u64;
    if size > 16 * 1024 * 1024 {
        return None;
    }
    let size = size as usize;
    if size <= 4 {
        Some((typ, count, src.get(entry + 8..entry + 8 + size)?))
    } else {
        let offset = read_u32(src, entry + 8, little)? as usize;
        Some((typ, count, src.get(offset..offset + size)?))
    }
}

fn pad_even(out: &mut Vec<u8>) {
    if out.len() % 2 == 1 {
        out.push(0);
    }
}

/// Append a field value (out of line when it exceeds four bytes) and push the
/// 12-byte entry into `entries`.
fn push_entry(
    out: &mut Vec<u8>,
    entries: &mut Vec<[u8; 12]>,
    tag: u16,
    typ: u16,
    count: u32,
    payload: &[u8],
    little: bool,
) {
    let mut entry = [0u8; 12];
    let tag_bytes = if little {
        tag.to_le_bytes()
    } else {
        tag.to_be_bytes()
    };
    let typ_bytes = if little {
        typ.to_le_bytes()
    } else {
        typ.to_be_bytes()
    };
    let count_bytes = if little {
        count.to_le_bytes()
    } else {
        count.to_be_bytes()
    };
    entry[0..2].copy_from_slice(&tag_bytes);
    entry[2..4].copy_from_slice(&typ_bytes);
    entry[4..8].copy_from_slice(&count_bytes);
    if payload.len() <= 4 {
        entry[8..8 + payload.len()].copy_from_slice(payload);
    } else {
        pad_even(out);
        let offset = out.len() as u32;
        out.extend_from_slice(payload);
        let offset_bytes = if little {
            offset.to_le_bytes()
        } else {
            offset.to_be_bytes()
        };
        entry[8..12].copy_from_slice(&offset_bytes);
    }
    entries.push(entry);
}

fn push_pointer(
    out: &mut Vec<u8>,
    entries: &mut Vec<[u8; 12]>,
    tag: u16,
    target: u32,
    little: bool,
) {
    let payload = if little {
        target.to_le_bytes()
    } else {
        target.to_be_bytes()
    };
    push_entry(out, entries, tag, 4, 1, &payload, little);
}

fn write_ifd(out: &mut Vec<u8>, entries: &[[u8; 12]], next: u32, little: bool) {
    let count_bytes = if little {
        (entries.len() as u16).to_le_bytes()
    } else {
        (entries.len() as u16).to_be_bytes()
    };
    out.extend_from_slice(&count_bytes);
    for entry in entries {
        out.extend_from_slice(entry);
    }
    let next_bytes = if little {
        next.to_le_bytes()
    } else {
        next.to_be_bytes()
    };
    out.extend_from_slice(&next_bytes);
}

/// Copy one IFD into `out`, rewriting out-of-line offsets. Nested IFD pointers
/// are dropped because their targets are not copied.
fn append_ifd(
    out: &mut Vec<u8>,
    src: &[u8],
    src_ifd: usize,
    src_little: bool,
    dst_little: bool,
) -> Option<u32> {
    let count = read_u16(src, src_ifd, src_little)? as usize;
    if count > 1000 || src_ifd + 2 + count * 12 > src.len() {
        return None;
    }
    let mut entries: Vec<[u8; 12]> = Vec::with_capacity(count);
    for index in 0..count {
        let entry = src_ifd + 2 + index * 12;
        let tag = read_u16(src, entry, src_little)?;
        if matches!(tag, TAG_EXIF_IFD | TAG_GPS_INFO | TAG_INTEROP_IFD) {
            continue;
        }
        let (typ, value_count, payload) = entry_payload(src, entry, src_little)?;
        let payload = if src_little == dst_little {
            payload.to_vec()
        } else {
            swap_endian(payload, swap_element(typ))
        };
        push_entry(
            out,
            &mut entries,
            tag,
            typ,
            value_count,
            &payload,
            dst_little,
        );
    }
    entries.sort_by_key(|entry| read_u16(entry, 0, dst_little).unwrap_or(0));
    pad_even(out);
    let offset = out.len() as u32;
    write_ifd(out, &entries, 0, dst_little);
    Some(offset)
}

/// Embed the camera EXIF block into an already encoded classic TIFF.
///
/// Returns false when the container is not a classic TIFF this writer can
/// safely extend; callers must then report that metadata was not written.
pub fn write_tiff_exif(file: &mut Vec<u8>, exif: &[u8]) -> bool {
    let Some((Endianness(little), ifd0)) = parse_header(file) else {
        return false;
    };
    let Some((Endianness(src_little), src_ifd0)) = parse_header(exif) else {
        return false;
    };
    let Some(count) = read_u16(file, ifd0, little).map(|count| count as usize) else {
        return false;
    };
    if count > 1000 || ifd0 + 2 + count * 12 + 4 > file.len() {
        return false;
    }
    let next = read_u32(file, ifd0 + 2 + count * 12, little).unwrap_or(0);

    let exif_ifd = find_entry(exif, src_ifd0, TAG_EXIF_IFD, src_little)
        .and_then(|entry| read_u32(exif, entry + 8, src_little))
        .map(|offset| offset as usize)
        .filter(|offset| *offset + 2 <= exif.len());

    let mut entries: Vec<[u8; 12]> = (0..count)
        .filter_map(|index| {
            let start = ifd0 + 2 + index * 12;
            let tag = read_u16(file, start, little)?;
            if tag == TAG_EXIF_IFD {
                return None;
            }
            let mut entry = [0u8; 12];
            entry.copy_from_slice(&file[start..start + 12]);
            Some(entry)
        })
        .collect();

    let mut out = file.clone();
    pad_even(&mut out);

    if let Some(src_ifd) = exif_ifd {
        if let Some(offset) = append_ifd(&mut out, exif, src_ifd, src_little, little) {
            push_pointer(&mut out, &mut entries, TAG_EXIF_IFD, offset, little);
        }
    }

    // Keep the location block for the Keep policy; Privacy already dropped the
    // pointer in `prepare_exif`, so there is nothing left to copy there.
    let gps_ifd = find_entry(exif, src_ifd0, TAG_GPS_INFO, src_little)
        .and_then(|entry| read_u32(exif, entry + 8, src_little))
        .map(|offset| offset as usize)
        .filter(|offset| *offset + 2 <= exif.len());
    if let Some(src_ifd) = gps_ifd {
        if let Some(offset) = append_ifd(&mut out, exif, src_ifd, src_little, little) {
            push_pointer(&mut out, &mut entries, TAG_GPS_INFO, offset, little);
        }
    }

    for tag in [
        TAG_MAKE,
        TAG_MODEL,
        TAG_SOFTWARE,
        TAG_DATETIME,
        TAG_ARTIST,
        TAG_COPYRIGHT,
    ] {
        if entries
            .iter()
            .any(|entry| read_u16(entry, 0, little) == Some(tag))
        {
            continue;
        }
        let Some(entry) = find_entry(exif, src_ifd0, tag, src_little) else {
            continue;
        };
        let Some((typ, value_count, payload)) = entry_payload(exif, entry, src_little) else {
            continue;
        };
        let payload = if src_little == little {
            payload.to_vec()
        } else {
            swap_endian(payload, swap_element(typ))
        };
        push_entry(
            &mut out,
            &mut entries,
            tag,
            typ,
            value_count,
            &payload,
            little,
        );
    }

    entries.sort_by_key(|entry| read_u16(entry, 0, little).unwrap_or(0));
    pad_even(&mut out);
    let new_ifd = out.len() as u32;
    write_ifd(&mut out, &entries, next, little);
    let header_offset = if little {
        new_ifd.to_le_bytes()
    } else {
        new_ifd.to_be_bytes()
    };
    out[4..8].copy_from_slice(&header_offset);
    *file = out;
    true
}

/// Read the ICC profile embedded in a TIFF buffer.
///
/// `image`'s TIFF encoder writes the profile as a BYTE array and its decoder
/// then fails to read it back, so imports use this instead.
pub fn tiff_icc_from_bytes(tiff: &[u8]) -> Option<Vec<u8>> {
    let (Endianness(little), ifd0) = parse_header(tiff)?;
    let count = read_u16(tiff, ifd0, little)? as usize;
    if count > 1000 || ifd0 + 2 + count * 12 > tiff.len() {
        return None;
    }
    for index in 0..count {
        let entry = ifd0 + 2 + index * 12;
        if read_u16(tiff, entry, little)? != TAG_ICC_PROFILE {
            continue;
        }
        let typ = read_u16(tiff, entry + 2, little)?;
        let value_count = read_u32(tiff, entry + 4, little)? as usize;
        if type_size(typ)? != 1 || value_count == 0 || value_count > 16 * 1024 * 1024 {
            return None;
        }
        if value_count <= 4 {
            return Some(tiff.get(entry + 8..entry + 8 + value_count)?.to_vec());
        }
        let offset = read_u32(tiff, entry + 8, little)? as usize;
        return Some(tiff.get(offset..offset + value_count)?.to_vec());
    }
    None
}

/// Same as [`tiff_icc_from_bytes`], but seeks so multi-megapixel exports are
/// not read into memory just to find the profile.
pub fn tiff_file_icc(path: &Path) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let mut header = [0u8; 8];
    file.read_exact(&mut header).ok()?;
    let little = match &header[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    if read_u16(&header, 2, little)? != 42 {
        return None;
    }
    let ifd0 = read_u32(&header, 4, little)? as u64;
    file.seek(SeekFrom::Start(ifd0)).ok()?;
    let mut count_bytes = [0u8; 2];
    file.read_exact(&mut count_bytes).ok()?;
    let count = read_u16(&count_bytes, 0, little)? as usize;
    if count > 1000 {
        return None;
    }
    let mut entries = vec![0u8; count * 12];
    file.read_exact(&mut entries).ok()?;
    for index in 0..count {
        let entry = index * 12;
        if read_u16(&entries, entry, little)? != TAG_ICC_PROFILE {
            continue;
        }
        let typ = read_u16(&entries, entry + 2, little)?;
        let value_count = read_u32(&entries, entry + 4, little)? as usize;
        if type_size(typ)? != 1 || value_count == 0 || value_count > 16 * 1024 * 1024 {
            return None;
        }
        if value_count <= 4 {
            return Some(entries.get(entry + 8..entry + 8 + value_count)?.to_vec());
        }
        let offset = read_u32(&entries, entry + 8, little)? as u64;
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut data = vec![0u8; value_count];
        file.read_exact(&mut data).ok()?;
        return Some(data);
    }
    None
}

/// True when the exported file's IFD0 carries `tag`. Reads only the header and
/// the IFD, so it stays cheap for large files.
pub fn tiff_file_has_tag(path: &Path, tag: u16) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut header = [0u8; 8];
    if file.read_exact(&mut header).is_err() {
        return false;
    }
    let little = match &header[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return false,
    };
    let magic = if little {
        u16::from_le_bytes([header[2], header[3]])
    } else {
        u16::from_be_bytes([header[2], header[3]])
    };
    if magic != 42 {
        return false;
    }
    let ifd0 = if little {
        u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as u64
    } else {
        u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as u64
    };
    if file.seek(SeekFrom::Start(ifd0)).is_err() {
        return false;
    }
    let mut count_bytes = [0u8; 2];
    if file.read_exact(&mut count_bytes).is_err() {
        return false;
    }
    let count = if little {
        u16::from_le_bytes(count_bytes)
    } else {
        u16::from_be_bytes(count_bytes)
    } as usize;
    if count > 1000 {
        return false;
    }
    let mut entries = vec![0u8; count * 12];
    if file.read_exact(&mut entries).is_err() {
        return false;
    }
    (0..count).any(|index| read_u16(&entries, index * 12, little) == Some(tag))
}

/// True when the exported file's IFD0 points at an Exif IFD.
pub fn tiff_file_has_exif(path: &Path) -> bool {
    tiff_file_has_tag(path, TAG_EXIF_IFD)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a little-endian TIFF block: IFD0 with the given tags, then an Exif
    /// sub-IFD holding the thumbnail tags.
    fn block(entries: &[(u16, u32)], sub_entries: &[(u16, u32)]) -> Vec<u8> {
        let ifd0_offset = 8usize;
        let mut out = Vec::new();
        out.extend_from_slice(b"II*\0");
        out.extend_from_slice(&(ifd0_offset as u32).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, value) in entries {
            out.extend_from_slice(&tag.to_le_bytes());
            out.extend_from_slice(&4u16.to_le_bytes()); // type LONG
            out.extend_from_slice(&1u32.to_le_bytes()); // count
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
        out.extend_from_slice(&(sub_entries.len() as u16).to_le_bytes());
        for (tag, value) in sub_entries {
            out.extend_from_slice(&tag.to_le_bytes());
            out.extend_from_slice(&4u16.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&0u32.to_le_bytes());
        out
    }

    fn orientation_value(tiff: &[u8]) -> u16 {
        let (Endianness(little), ifd0) = parse_header(tiff).unwrap();
        let entry = find_entry(tiff, ifd0, TAG_ORIENTATION, little).unwrap();
        u16::from_le_bytes([tiff[entry + 8], tiff[entry + 9]])
    }

    fn count_entries(tiff: &[u8], ifd: usize) -> usize {
        let little = matches!(&tiff[0..2], b"II");
        read_u16(tiff, ifd, little).unwrap() as usize
    }

    /// A camera-style EXIF block: IFD0 carries Make and an Exif IFD pointer;
    /// the Exif IFD carries ExposureTime as an out-of-line RATIONAL.
    fn camera_exif() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"II* ");
        out.extend_from_slice(&8u32.to_le_bytes());
        let make_offset = 8 + 2 + 2 * 12 + 4;
        let exif_ifd = make_offset + 6;
        let rational = exif_ifd + 2 + 12 + 4;
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&TAG_MAKE.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes()); // ASCII
        out.extend_from_slice(&6u32.to_le_bytes());
        out.extend_from_slice(&(make_offset as u32).to_le_bytes());
        out.extend_from_slice(&TAG_EXIF_IFD.to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&(exif_ifd as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(b"Nikon ");
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&0x829Au16.to_le_bytes()); // ExposureTime
        out.extend_from_slice(&5u16.to_le_bytes()); // RATIONAL
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&(rational as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&125u32.to_le_bytes());
        out
    }

    fn encoded_tiff() -> Vec<u8> {
        use image::codecs::tiff::TiffEncoder;
        use image::{ExtendedColorType, ImageEncoder};
        let mut cursor = std::io::Cursor::new(Vec::new());
        TiffEncoder::new(&mut cursor)
            .write_image(
                &[10u8, 20, 30, 255, 40, 50, 60, 255],
                2,
                1,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        cursor.into_inner()
    }

    #[test]
    fn tiff_exif_is_appended_and_still_decodes() {
        let exif = camera_exif();
        let mut tiff = encoded_tiff();
        let before = tiff.len();
        assert!(write_tiff_exif(&mut tiff, &exif));
        assert!(tiff.len() > before, "the rewrite appends a new IFD");
        let decoded = image::load_from_memory(&tiff).expect("pixels still decode");
        assert_eq!((decoded.width(), decoded.height()), (2, 1));

        let (Endianness(little), ifd0) = parse_header(&tiff).unwrap();
        let make = find_entry(&tiff, ifd0, TAG_MAKE, little).expect("Make copied into IFD0");
        assert_eq!(read_u32(&tiff, make + 4, little).unwrap(), 6);
        let offset = read_u32(&tiff, make + 8, little).unwrap() as usize;
        assert_eq!(&tiff[offset..offset + 6], b"Nikon ");

        let pointer = find_entry(&tiff, ifd0, TAG_EXIF_IFD, little).expect("Exif IFD pointer");
        let exif_ifd = read_u32(&tiff, pointer + 8, little).unwrap() as usize;
        let exposure = find_entry(&tiff, exif_ifd, 0x829A, little).expect("ExposureTime copied");
        let value = read_u32(&tiff, exposure + 8, little).unwrap() as usize;
        assert_eq!(read_u32(&tiff, value, little).unwrap(), 1);
        assert_eq!(read_u32(&tiff, value + 4, little).unwrap(), 125);
    }

    #[test]
    fn tiff_exif_rejects_files_it_cannot_parse() {
        let exif = camera_exif();
        let mut not_a_tiff = b"BM.............".to_vec();
        let untouched = not_a_tiff.clone();
        assert!(!write_tiff_exif(&mut not_a_tiff, &exif));
        assert_eq!(not_a_tiff, untouched);
    }

    #[test]
    fn tiff_file_check_reads_the_pointer_back() {
        let exif = camera_exif();
        let mut tiff = encoded_tiff();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe.tiff");
        std::fs::write(&path, &tiff).unwrap();
        assert!(!tiff_file_has_exif(&path));
        assert!(write_tiff_exif(&mut tiff, &exif));
        std::fs::write(&path, &tiff).unwrap();
        assert!(tiff_file_has_exif(&path));
    }

    #[test]
    fn orientation_is_reset_and_thumbnail_is_dropped() {
        let exif_ifd = 8 + 2 + 4 * 12 + 4;
        let source = block(
            &[
                (TAG_ORIENTATION, 6),
                (0x0131, 1234),
                (0x0110, 2222),
                (TAG_EXIF_IFD, exif_ifd as u32),
            ],
            &[(TAG_THUMBNAIL_OFFSET, 999), (TAG_THUMBNAIL_LENGTH, 4096)],
        );
        assert_eq!(orientation_value(&source), 6);
        assert!(has_thumbnail(&source));

        let prepared = prepare_exif(&source, MetadataPolicy::Keep).unwrap();
        assert_eq!(orientation_value(&prepared), 1);
        let (Endianness(little), ifd0) = parse_header(&prepared).unwrap();
        // The thumbnail tags live in the sub-IFD, so IFD0 keeps all four entries.
        assert_eq!(count_entries(&prepared, ifd0), 4);
        assert!(find_entry(&prepared, ifd0, TAG_EXIF_IFD, little).is_some());
        let sub = read_u32(
            &prepared,
            find_entry(&prepared, ifd0, TAG_EXIF_IFD, little).unwrap() + 8,
            little,
        )
        .unwrap() as usize;
        assert_eq!(count_entries(&prepared, sub), 0);
        assert!(!has_thumbnail(&prepared));
    }

    #[test]
    fn privacy_policy_also_removes_the_gps_pointer() {
        let source = block(
            &[(TAG_ORIENTATION, 3), (TAG_GPS_INFO, 5000), (0x0132, 7)],
            &[],
        );
        assert!(has_gps(&source));
        let kept = prepare_exif(&source, MetadataPolicy::Keep).unwrap();
        let (Endianness(little), ifd0) = parse_header(&kept).unwrap();
        assert!(find_entry(&kept, ifd0, TAG_GPS_INFO, little).is_some());

        let private = prepare_exif(&source, MetadataPolicy::Privacy).unwrap();
        let (Endianness(little), ifd0) = parse_header(&private).unwrap();
        assert_eq!(count_entries(&private, ifd0), 2);
        assert!(!has_gps(&private));
        // Remaining entries keep their values after the shift.
        assert_eq!(
            read_u32(
                &private,
                find_entry(&private, ifd0, 0x0132, little).unwrap() + 8,
                little
            ),
            Some(7)
        );
        // The next-IFD pointer still reads as "no more IFDs".
        assert_eq!(read_u32(&private, ifd0 + 2 + 2 * 12, little), Some(0));
    }

    #[test]
    fn none_policy_and_garbage_input_are_rejected() {
        let source = block(&[(TAG_ORIENTATION, 8)], &[]);
        assert!(prepare_exif(&source, MetadataPolicy::None).is_none());
        assert!(prepare_exif(b"not exif", MetadataPolicy::Keep).is_none());
        assert!(prepare_exif(b"II", MetadataPolicy::Keep).is_none());
        // A block whose IFD points past the end is refused.
        let mut broken = block(&[(TAG_ORIENTATION, 1)], &[]);
        broken[4..8].copy_from_slice(&9000u32.to_le_bytes());
        assert!(prepare_exif(&broken, MetadataPolicy::Keep).is_none());
        assert!(!has_gps(&broken));
    }

    #[test]
    fn big_endian_blocks_are_understood() {
        let mut big = Vec::new();
        big.extend_from_slice(b"MM\0*");
        big.extend_from_slice(&8u32.to_be_bytes());
        big.extend_from_slice(&2u16.to_be_bytes());
        for (tag, value) in [(TAG_ORIENTATION, 5u32), (TAG_GPS_INFO, 40u32)] {
            big.extend_from_slice(&tag.to_be_bytes());
            big.extend_from_slice(&4u16.to_be_bytes());
            big.extend_from_slice(&1u32.to_be_bytes());
            big.extend_from_slice(&value.to_be_bytes());
        }
        big.extend_from_slice(&0u32.to_be_bytes());
        assert!(has_gps(&big));
        let prepared = prepare_exif(&big, MetadataPolicy::Privacy).unwrap();
        let (Endianness(little), ifd0) = parse_header(&prepared).unwrap();
        assert!(!little);
        assert_eq!(count_entries(&prepared, ifd0), 1);
        assert_eq!(
            read_u16(
                &prepared,
                find_entry(&prepared, ifd0, TAG_ORIENTATION, false).unwrap() + 8,
                false
            ),
            Some(1)
        );
    }
}
