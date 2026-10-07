//! One bounded raw stream contains exactly the four validated package objects

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    time::Duration,
};

use cocobeat_media::{
    MAX_RECEIVED_PACKAGE_BYTES, PACKAGE_OBJECT_LIMITS, PACKAGE_OBJECT_NAMES, ReceivedPackage,
    ValidatedPackage,
};
use quinn::{Connection, RecvStream};

use crate::wire::{self, Identity, ResourceObject};

pub(crate) const TRANSFER_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const PROGRESS_TIMEOUT: Duration = Duration::from_secs(30);
const CHUNK_BYTES: usize = 64 * 1024;
const MAGIC: &[u8; 8] = b"CCBRSC02";

pub(crate) fn package_hash(identity: &Identity) -> Result<[u8; 32], String> {
    let hash = identity
        .content_id
        .strip_prefix("package-blake3:")
        .filter(|hash| hash.len() == 64)
        .ok_or("received package identity must contain a complete BLAKE3 hash")?;
    Ok(*blake3::Hash::from_hex(hash)
        .map_err(|_| "received package identity hash is invalid")?
        .as_bytes())
}

pub(crate) fn validate_objects(objects: &[ResourceObject; 4]) -> Result<(), String> {
    let mut total = 0_u64;
    for (object, limit) in objects.iter().zip(PACKAGE_OBJECT_LIMITS) {
        if !(1..=limit).contains(&object.bytes) {
            return Err("resource descriptor exceeds its object size limit".into());
        }
        total = total
            .checked_add(object.bytes)
            .filter(|total| *total <= MAX_RECEIVED_PACKAGE_BYTES)
            .ok_or("resource descriptor total exceeds package size limit")?;
    }
    Ok(())
}

pub(crate) struct Source {
    pub objects: [ResourceObject; 4],
    files: [File; 4],
}

impl Source {
    pub fn open(root: &Path, expected: &Identity) -> Result<Self, String> {
        let (package, references) = cocobeat_media::validate_package_objects(root)?;
        if package.manifest.package_hash != package_hash(expected)? {
            return Err("resource source identity changed after session preparation".into());
        }
        let mut files = Vec::with_capacity(4);
        let mut objects = Vec::with_capacity(4);
        let mut buffer = [0; CHUNK_BYTES];
        for (index, name) in PACKAGE_OBJECT_NAMES.iter().enumerate() {
            let path = root.join(name);
            if !std::fs::symlink_metadata(&path)
                .map_err(|_| "inspect resource source failed")?
                .is_file()
            {
                return Err("resource source must be a regular object, not a symlink".into());
            }
            let mut file = File::open(path).map_err(|_| "open resource source failed")?;
            let metadata = file
                .metadata()
                .map_err(|_| "inspect opened resource failed")?;
            let length = metadata.len();
            if !metadata.is_file() || !(1..=PACKAGE_OBJECT_LIMITS[index]).contains(&length) {
                return Err("resource source exceeds its object size limit".into());
            }
            let mut hash = blake3::Hasher::new();
            let mut read = 0_u64;
            loop {
                let count = file
                    .read(&mut buffer)
                    .map_err(|_| "hash resource source failed")?;
                if count == 0 {
                    break;
                }
                read = read
                    .checked_add(count as u64)
                    .filter(|read| *read <= length)
                    .ok_or("resource source grew while hashing")?;
                hash.update(&buffer[..count]);
            }
            if read != length {
                return Err("resource source shrank while hashing".into());
            }
            let hash = *hash.finalize().as_bytes();
            if references[index].blake3 != hash || references[index].byte_len != length {
                return Err("resource source object changed after validation".into());
            }
            file.seek(SeekFrom::Start(0))
                .map_err(|_| "rewind resource source failed")?;
            files.push(file);
            objects.push(ResourceObject {
                bytes: length,
                blake3: hash,
            });
        }
        Ok(Self {
            objects: objects
                .try_into()
                .map_err(|_| "resource object count differs")?,
            files: files
                .try_into()
                .map_err(|_| "resource file count differs")?,
        })
    }

    pub async fn send(mut self, connection: &Connection, epoch: u64) -> Result<(), String> {
        let mut stream = connection
            .open_uni()
            .await
            .map_err(|_| "open package stream failed")?;
        let mut header = [0; 16];
        header[..8].copy_from_slice(MAGIC);
        header[8..].copy_from_slice(&epoch.to_be_bytes());
        tokio::time::timeout(PROGRESS_TIMEOUT, stream.write_all(&header))
            .await
            .map_err(|_| "package stream header send timed out")?
            .map_err(|_| "send package stream header failed")?;
        let mut buffer = [0; CHUNK_BYTES];
        for (index, file) in self.files.iter_mut().enumerate() {
            let mut remaining = self.objects[index].bytes;
            let mut hash = blake3::Hasher::new();
            while remaining != 0 {
                let amount = remaining.min(CHUNK_BYTES as u64) as usize;
                file.read_exact(&mut buffer[..amount])
                    .map_err(|_| "resource source truncated during send")?;
                hash.update(&buffer[..amount]);
                tokio::time::timeout(PROGRESS_TIMEOUT, stream.write_all(&buffer[..amount]))
                    .await
                    .map_err(|_| "resource send progress timed out")?
                    .map_err(|_| "send resource bytes failed")?;
                remaining -= amount as u64;
            }
            let mut extra = [0];
            if file
                .read(&mut extra)
                .map_err(|_| "read resource source EOF failed")?
                != 0
                || hash.finalize().as_bytes() != &self.objects[index].blake3
            {
                return Err("resource source changed during transfer".into());
            }
        }
        stream
            .finish()
            .map_err(|_| "finish package stream failed".to_owned())
    }
}

pub(crate) async fn receive(
    stream: &mut RecvStream,
    destination: &Path,
    expected: &Identity,
    epoch: u64,
    objects: [ResourceObject; 4],
) -> Result<ValidatedPackage, String> {
    validate_objects(&objects)?;
    let mut package = ReceivedPackage::new(
        destination,
        package_hash(expected)?,
        objects.map(|object| object.bytes),
    )?;
    let mut header = [0; 16];
    tokio::time::timeout(PROGRESS_TIMEOUT, stream.read_exact(&mut header))
        .await
        .map_err(|_| "package stream header timed out")?
        .map_err(|_| "package stream header is truncated")?;
    if &header[..8] != MAGIC || header[8..] != epoch.to_be_bytes() {
        return Err("package stream kind or epoch differs".into());
    }
    let mut buffer = [0; CHUNK_BYTES];
    for (index, object) in objects.iter().enumerate() {
        let mut remaining = object.bytes;
        let mut hash = blake3::Hasher::new();
        while remaining != 0 {
            let amount = remaining.min(CHUNK_BYTES as u64) as usize;
            tokio::time::timeout(PROGRESS_TIMEOUT, stream.read_exact(&mut buffer[..amount]))
                .await
                .map_err(|_| "resource receive progress timed out")?
                .map_err(|_| "resource object is truncated")?;
            hash.update(&buffer[..amount]);
            package.write(index, &buffer[..amount])?;
            remaining -= amount as u64;
        }
        if hash.finalize().as_bytes() != &object.blake3 {
            return Err("received resource object BLAKE3 differs".into());
        }
    }
    wire::ensure_eof(stream).await?;
    package.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_identity_requires_exact_package_hash() {
        let mut identity = Identity {
            content_id: format!("package-blake3:{}", "07".repeat(32)),
            canonical_frames: 1,
            content_schema: 1,
            ruleset_id: "duo-watermark-v1".into(),
            stage_compiler_version: None,
        };
        assert_eq!(package_hash(&identity).unwrap(), [7; 32]);
        let valid = PACKAGE_OBJECT_LIMITS.map(|bytes| ResourceObject {
            bytes,
            blake3: [0; 32],
        });
        validate_objects(&valid).unwrap();
        for index in 0..4 {
            for bytes in [0, PACKAGE_OBJECT_LIMITS[index] + 1, u64::MAX] {
                let mut invalid = valid;
                invalid[index].bytes = bytes;
                assert!(validate_objects(&invalid).is_err());
            }
        }
        for invalid in [
            "package",
            "package-blake3:07",
            &format!("package-blake3:{}", "z".repeat(64)),
            &format!("package-blake3:{}0", "07".repeat(32)),
        ] {
            identity.content_id = invalid.into();
            assert!(package_hash(&identity).is_err());
        }
    }
}
