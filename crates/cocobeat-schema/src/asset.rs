//! Identity of the exact encoded bytes stored in a song package object

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetRef {
    pub file_name: String,
    pub byte_len: u64,
    pub blake3: [u8; 32],
}
