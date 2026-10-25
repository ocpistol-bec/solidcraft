//! The [MS-CFB] compound-file container of a document (spec clause 7).
//!
//! The container is read into memory whole: every storage with its CLSID and every stream with
//! its bytes, keyed by a `/`-separated path. Writing builds a fresh version-3 compound file from
//! that model, so unknown storages and streams survive a round trip untouched [W-01].

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use crate::error::{CorlissError, Result};
use crate::guid::Guid;

/// Directory metadata of a storage or stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    /// CLSID of the entry (meaningful for storages; the root CLSID names the document type).
    pub clsid: Guid,
    /// User-defined state bits.
    pub state_bits: u32,
    /// Creation time (storages only; streams store none).
    pub created: SystemTime,
    /// Modification time (storages only).
    pub modified: SystemTime,
}

/// One storage or stream below the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerEntry {
    /// A storage (directory).
    Storage(EntryInfo),
    /// A stream with its whole content.
    Stream {
        /// Directory metadata.
        info: EntryInfo,
        /// Stream bytes.
        data: Vec<u8>,
    },
}

/// An in-memory compound file: the root entry and every entry below it by path.
///
/// ```no_run
/// let bytes = std::fs::read("part.ipt").unwrap();
/// let container = corliss::Container::from_bytes(&bytes).unwrap();
/// let seg_info = container.stream("RSeStorage/RSeSegInfo").unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    root: EntryInfo,
    entries: BTreeMap<String, ContainerEntry>,
}

impl Container {
    /// Reads a compound file from memory.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let io_err = |e: std::io::Error| CorlissError::io("read", "compound file", &e);
        let mut file = cfb::CompoundFile::open(Cursor::new(bytes)).map_err(io_err)?;
        let root = entry_info(&file.root_entry());
        let listed: Vec<(String, cfb::Entry)> = file
            .walk()
            .filter(|e| !e.is_root())
            .map(|e| (path_key(e.path()), e))
            .collect();
        let mut entries = BTreeMap::new();
        for (key, entry) in listed {
            let value = read_entry(&mut file, &entry).map_err(io_err)?;
            entries.insert(key, value);
        }
        Ok(Container { root, entries })
    }

    /// Writes the container as a version-3 compound file (512-byte sectors, spec 7.1).
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let io_err = |e: std::io::Error| CorlissError::io("write", "compound file", &e);
        let cursor = Cursor::new(Vec::new());
        let mut file =
            cfb::CompoundFile::create_with_version(cfb::Version::V3, cursor).map_err(io_err)?;
        write_info(&mut file, Path::new("/"), &self.root).map_err(io_err)?;
        for (key, entry) in &self.entries {
            write_entry(&mut file, &entry_path(key), entry).map_err(io_err)?;
        }
        file.flush().map_err(io_err)?;
        Ok(file.into_inner().into_inner())
    }

    /// The root CLSID, which identifies the document type (spec Table 7-1).
    pub fn root_clsid(&self) -> Guid {
        self.root.clsid
    }

    /// All entries below the root, in path order.
    pub fn entries(&self) -> &BTreeMap<String, ContainerEntry> {
        &self.entries
    }

    /// The bytes of the stream at `path`, if the container has such a stream.
    pub fn stream(&self, path: &str) -> Option<&[u8]> {
        match self.entries.get(path)? {
            ContainerEntry::Stream { data, .. } => Some(data),
            ContainerEntry::Storage(_) => None,
        }
    }

    /// Paths of the streams directly inside the storage `storage` ("" for the root).
    pub fn streams_in(&self, storage: &str) -> impl Iterator<Item = &str> {
        let prefix = if storage.is_empty() {
            String::new()
        } else {
            format!("{storage}/")
        };
        self.entries.iter().filter_map(move |(key, entry)| {
            let name = key.strip_prefix(&prefix)?;
            let direct = !name.contains('/') && matches!(entry, ContainerEntry::Stream { .. });
            direct.then_some(key.as_str())
        })
    }

    /// Replaces the bytes of an existing stream; fails when `path` is not a stream.
    pub fn replace_stream(&mut self, path: &str, bytes: Vec<u8>) -> Result<()> {
        match self.entries.get_mut(path) {
            Some(ContainerEntry::Stream { data, .. }) => {
                *data = bytes;
                Ok(())
            }
            _ => Err(CorlissError::missing(
                "write",
                "compound file",
                format!("stream {path}"),
            )),
        }
    }
}

fn entry_info(entry: &cfb::Entry) -> EntryInfo {
    EntryInfo {
        clsid: Guid::from_bytes(entry.clsid().to_bytes_le()),
        state_bits: entry.state_bits(),
        created: entry.created(),
        modified: entry.modified(),
    }
}

fn read_entry<F: Read + std::io::Seek>(
    file: &mut cfb::CompoundFile<F>,
    entry: &cfb::Entry,
) -> std::io::Result<ContainerEntry> {
    let info = entry_info(entry);
    if !entry.is_stream() {
        return Ok(ContainerEntry::Storage(info));
    }
    let mut data = Vec::with_capacity(usize::try_from(entry.len()).unwrap_or(0));
    file.open_stream(entry.path())?.read_to_end(&mut data)?;
    Ok(ContainerEntry::Stream { info, data })
}

/// Joins the components of a cfb path with `/` whatever the host separator is.
fn path_key(path: &Path) -> String {
    let names: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    names.join("/")
}

fn entry_path(key: &str) -> PathBuf {
    let mut path = PathBuf::from("/");
    for name in key.split('/') {
        path.push(name);
    }
    path
}

type MemFile = cfb::CompoundFile<Cursor<Vec<u8>>>;

fn write_entry(file: &mut MemFile, path: &Path, entry: &ContainerEntry) -> std::io::Result<()> {
    match entry {
        ContainerEntry::Storage(info) => {
            file.create_storage(path)?;
            write_info(file, path, info)
        }
        ContainerEntry::Stream { info, data } => {
            file.create_stream(path)?.write_all(data)?;
            file.set_state_bits(path, info.state_bits)
        }
    }
}

fn write_info(file: &mut MemFile, path: &Path, info: &EntryInfo) -> std::io::Result<()> {
    let clsid = uuid::Uuid::from_bytes_le(*info.clsid.as_bytes());
    file.set_storage_clsid(path, clsid)?;
    file.set_state_bits(path, info.state_bits)?;
    file.set_created_time(path, info.created)?;
    file.set_modified_time(path, info.modified)
}
