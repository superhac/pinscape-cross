//! UF2 firmware images: parsing, validation and flash planning (no I/O).
//!
//! UF2 (<https://github.com/microsoft/uf2>) is a sequence of 512-byte blocks,
//! each carrying up to 476 bytes of payload for one target address. The Pico
//! boot ROM accepts it by drag-and-drop; we also use it to drive PICOBOOT
//! flashing ourselves, which is why this module turns an image into a
//! [`FlashPlan`]: the exact 4 KB sectors to erase and 256-byte pages to write.
//!
//! Safety rule: a plan only ever touches the sectors the image covers.
//! Pinscape keeps its flash file system (the config files) at the **top** of
//! flash, so in addition we refuse images that reach the top sector of any
//! plausible flash size (see [`PROTECTED_FLASH_SIZES`]).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

pub const BLOCK_SIZE: usize = 512;
pub const MAGIC_START0: u32 = 0x0A32_4655; // "UF2\n"
pub const MAGIC_START1: u32 = 0x9E5D_5157;
pub const MAGIC_END: u32 = 0x0AB1_6F30;
/// Largest payload a block can carry (512 − 32-byte header − 4-byte end magic).
pub const MAX_PAYLOAD: usize = 476;

/// Block flags.
pub mod flags {
    /// Not for main flash (e.g. comments); must not be written.
    pub const NOT_MAIN_FLASH: u32 = 0x0000_0001;
    pub const FILE_CONTAINER: u32 = 0x0000_1000;
    pub const FAMILY_ID_PRESENT: u32 = 0x0000_2000;
    pub const MD5_PRESENT: u32 = 0x0000_4000;
    pub const EXTENSION_TAGS_PRESENT: u32 = 0x0000_8000;
}

/// Family IDs used by Raspberry Pi parts (pico-sdk `uf2.h`).
pub mod family {
    pub const RP2040: u32 = 0xE48B_FF56;
    /// RP2350 "absolute": written at the given addresses regardless of partitions.
    pub const ABSOLUTE: u32 = 0xE48B_FF57;
    pub const DATA: u32 = 0xE48B_FF58;
    pub const RP2350_ARM_S: u32 = 0xE48B_FF59;
    pub const RP2350_RISCV: u32 = 0xE48B_FF5A;
    pub const RP2350_ARM_NS: u32 = 0xE48B_FF5B;
}

pub fn family_name(id: u32) -> &'static str {
    match id {
        family::RP2040 => "RP2040",
        family::ABSOLUTE => "RP2350 absolute",
        family::DATA => "data",
        family::RP2350_ARM_S => "RP2350 ARM (secure)",
        family::RP2350_RISCV => "RP2350 RISC-V",
        family::RP2350_ARM_NS => "RP2350 ARM (non-secure)",
        _ => "unknown",
    }
}

/// Start of the XIP flash window on both chips.
pub const FLASH_START: u32 = 0x1000_0000;
/// The XIP window is 16 MB (the largest flash either chip can address).
pub const FLASH_WINDOW: u32 = 16 * 1024 * 1024;
pub const SECTOR_SIZE: u32 = 4096;
pub const PAGE_SIZE: u32 = 256;

/// Flash sizes whose top sector we never touch. Pinscape's flash file system
/// grows down from the top of flash (`FlashStorage.cpp`), and we can't know
/// the board's flash size from the boot ROM on an RP2040, so we protect the
/// top sector at every size a Pinscape board may have (Pico: 2 MB, Pico 2: 4 MB).
pub const PROTECTED_FLASH_SIZES: [u32; 4] = [2 << 20, 4 << 20, 8 << 20, 16 << 20];

/// RP2350-E10 erratum workaround: picotool/the SDK prepend a dummy block in
/// the absolute family at this address (block 0 of a 2-block sequence) so the
/// RP2350 boot ROM handles the following image correctly. It is not real
/// data and must never be written by us (on a small flash it would alias into
/// the config area).
pub const E10_WORKAROUND_ADDR: u32 = 0x10FF_FF00;

/// Which Pico chip a boot device or image is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Chip {
    Rp2040,
    Rp2350,
}

impl Chip {
    /// From `DeviceIds::cpu_type` (2040 or 2350).
    pub fn from_cpu_type(cpu_type: u16) -> Option<Chip> {
        match cpu_type {
            2040 => Some(Chip::Rp2040),
            2350 => Some(Chip::Rp2350),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Chip::Rp2040 => "RP2040",
            Chip::Rp2350 => "RP2350",
        }
    }

    /// Families this chip's boot ROM would write to flash.
    pub fn accepts(self, fam: u32) -> bool {
        match self {
            Chip::Rp2040 => fam == family::RP2040,
            Chip::Rp2350 => matches!(
                fam,
                family::ABSOLUTE | family::RP2350_ARM_S | family::RP2350_RISCV | family::RP2350_ARM_NS
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Uf2Error {
    #[error("file is empty")]
    Empty,
    #[error("file size {0} is not a multiple of 512 bytes, so it isn't a UF2 file")]
    BadLength(usize),
    #[error("block {0}: bad UF2 magic number (not a UF2 file, or corrupted)")]
    BadMagic(usize),
    #[error("block {block}: payload size {size} is invalid")]
    BadPayloadSize { block: usize, size: u32 },
    #[error("block {block}: block number {no} is not below the block count {count}")]
    BadBlockNumber { block: usize, no: u32, count: u32 },
    #[error("{family}: expected {expected} blocks but found {found} (truncated or corrupted file)")]
    BlockCountMismatch { family: String, expected: u32, found: u32 },
    #[error("{family}: block number {no} appears more than once")]
    DuplicateBlock { family: String, no: u32 },
    #[error("the image contains no {0} firmware (wrong file for this Pico?)")]
    NoBlocksForChip(&'static str),
    #[error("block at {addr:#010x} (+{len}) is outside the flash address range")]
    OutsideFlash { addr: u32, len: u32 },
    #[error("block at {addr:#010x} is not 256-byte page aligned in a way that can be programmed")]
    Unaligned { addr: u32 },
    #[error("two blocks write the same flash bytes at {addr:#010x}")]
    Overlap { addr: u32 },
    #[error("the image writes the flash sector at {addr:#010x}, which holds the Pinscape config storage; refusing to erase it")]
    ProtectedSector { addr: u32 },
}

/// One decoded 512-byte block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub flags: u32,
    pub target_addr: u32,
    pub block_no: u32,
    pub num_blocks: u32,
    /// Family ID (or, without `FAMILY_ID_PRESENT`, the file size field).
    pub family: Option<u32>,
    pub data: Vec<u8>,
}

fn u32_at(b: &[u8], ofs: usize) -> u32 {
    u32::from_le_bytes([b[ofs], b[ofs + 1], b[ofs + 2], b[ofs + 3]])
}

impl Block {
    /// Decode one block; `index` is only used for error messages.
    pub fn parse(b: &[u8], index: usize) -> Result<Block, Uf2Error> {
        if b.len() != BLOCK_SIZE
            || u32_at(b, 0) != MAGIC_START0
            || u32_at(b, 4) != MAGIC_START1
            || u32_at(b, BLOCK_SIZE - 4) != MAGIC_END
        {
            return Err(Uf2Error::BadMagic(index));
        }
        let flags = u32_at(b, 8);
        let target_addr = u32_at(b, 12);
        let size = u32_at(b, 16);
        let block_no = u32_at(b, 20);
        let num_blocks = u32_at(b, 24);
        let fam = u32_at(b, 28);
        if size as usize > MAX_PAYLOAD {
            return Err(Uf2Error::BadPayloadSize { block: index, size });
        }
        if block_no >= num_blocks {
            return Err(Uf2Error::BadBlockNumber { block: index, no: block_no, count: num_blocks });
        }
        Ok(Block {
            flags,
            target_addr,
            block_no,
            num_blocks,
            family: (flags & flags::FAMILY_ID_PRESENT != 0).then_some(fam),
            data: b[32..32 + size as usize].to_vec(),
        })
    }

    /// Encode as a 512-byte block (used to build test images).
    pub fn encode(&self) -> [u8; BLOCK_SIZE] {
        let mut b = [0u8; BLOCK_SIZE];
        let mut put = |ofs: usize, v: u32| b[ofs..ofs + 4].copy_from_slice(&v.to_le_bytes());
        put(0, MAGIC_START0);
        put(4, MAGIC_START1);
        put(8, self.flags | if self.family.is_some() { flags::FAMILY_ID_PRESENT } else { 0 });
        put(12, self.target_addr);
        put(16, self.data.len() as u32);
        put(20, self.block_no);
        put(24, self.num_blocks);
        put(28, self.family.unwrap_or(0));
        put(BLOCK_SIZE - 4, MAGIC_END);
        b[32..32 + self.data.len()].copy_from_slice(&self.data);
        b
    }

    pub fn end_addr(&self) -> u64 {
        self.target_addr as u64 + self.data.len() as u64
    }

    fn is_flash_data(&self) -> bool {
        self.flags & (flags::NOT_MAIN_FLASH | flags::FILE_CONTAINER) == 0
    }

    /// The RP2350-E10 dummy block (see [`E10_WORKAROUND_ADDR`]).
    pub fn is_e10_workaround(&self) -> bool {
        self.family == Some(family::ABSOLUTE) && self.target_addr == E10_WORKAROUND_ADDR && self.num_blocks == 2
    }
}

/// What kind of program an image appears to be (by strings in its payload).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageKind {
    /// Pinscape Pico main firmware.
    Pinscape,
    /// Pinscape PWMWorker (for the Pico on an expansion board, not this device).
    PwmWorker,
    Unknown,
}

/// Per-family summary of an image.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilyInfo {
    pub id: u32,
    pub name: &'static str,
    pub blocks: u32,
    pub payload_bytes: u64,
    pub min_addr: u32,
    /// One past the last byte written.
    pub end_addr: u64,
}

/// Summary shown in the UI before installing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Uf2Info {
    pub file_bytes: u64,
    pub total_blocks: u32,
    pub families: Vec<FamilyInfo>,
    /// Chips this image can be installed on.
    pub chips: Vec<Chip>,
    pub kind: ImageKind,
    /// Contains the RP2350-E10 workaround block (ignored when flashing).
    pub e10_block: bool,
    pub warnings: Vec<String>,
}

/// A parsed, structurally valid UF2 file.
#[derive(Debug, Clone)]
pub struct Uf2Image {
    pub blocks: Vec<Block>,
    file_bytes: u64,
}

impl Uf2Image {
    /// Parse and validate the block structure: magic numbers, payload sizes,
    /// and that every block sequence (per family and block count) is complete.
    pub fn parse(bytes: &[u8]) -> Result<Uf2Image, Uf2Error> {
        if bytes.is_empty() {
            return Err(Uf2Error::Empty);
        }
        if !bytes.len().is_multiple_of(BLOCK_SIZE) {
            return Err(Uf2Error::BadLength(bytes.len()));
        }
        let blocks = bytes
            .chunks(BLOCK_SIZE)
            .enumerate()
            .map(|(i, b)| Block::parse(b, i))
            .collect::<Result<Vec<_>, _>>()?;

        // A file may concatenate several images (e.g. the E10 block + the
        // real image, or one image per chip); block numbers restart per image.
        // The E10 dummy block claims a count of 2 but stands alone by design.
        let mut seqs: BTreeMap<(Option<u32>, u32), BTreeSet<u32>> = BTreeMap::new();
        for b in blocks.iter().filter(|b| !b.is_e10_workaround()) {
            let fam_label = || b.family.map(family_name).unwrap_or("no family").to_string();
            if !seqs.entry((b.family, b.num_blocks)).or_default().insert(b.block_no) {
                return Err(Uf2Error::DuplicateBlock { family: fam_label(), no: b.block_no });
            }
        }
        for ((fam, count), nos) in &seqs {
            if nos.len() as u32 != *count {
                return Err(Uf2Error::BlockCountMismatch {
                    family: fam.map(family_name).unwrap_or("no family").to_string(),
                    expected: *count,
                    found: nos.len() as u32,
                });
            }
        }
        Ok(Uf2Image { blocks, file_bytes: bytes.len() as u64 })
    }

    pub fn info(&self) -> Uf2Info {
        let mut fams: BTreeMap<u32, FamilyInfo> = BTreeMap::new();
        let mut e10_block = false;
        let mut warnings = Vec::new();
        for b in &self.blocks {
            if b.is_e10_workaround() {
                e10_block = true;
                continue;
            }
            if !b.is_flash_data() {
                continue;
            }
            let Some(id) = b.family else { continue };
            let f = fams.entry(id).or_insert(FamilyInfo {
                id,
                name: family_name(id),
                blocks: 0,
                payload_bytes: 0,
                min_addr: u32::MAX,
                end_addr: 0,
            });
            f.blocks += 1;
            f.payload_bytes += b.data.len() as u64;
            f.min_addr = f.min_addr.min(b.target_addr);
            f.end_addr = f.end_addr.max(b.end_addr());
        }
        if self.blocks.iter().any(|b| b.family.is_none() && b.is_flash_data()) {
            warnings.push("Some blocks have no family ID; the Pico boot ROM ignores them.".into());
        }
        let families: Vec<FamilyInfo> = fams.into_values().collect();
        let chips: Vec<Chip> = [Chip::Rp2040, Chip::Rp2350]
            .into_iter()
            .filter(|c| families.iter().any(|f| c.accepts(f.id)))
            .collect();
        if chips.is_empty() {
            warnings.push("The image has no RP2040 or RP2350 firmware.".into());
        }
        for f in &families {
            if f.min_addr < FLASH_START || f.end_addr > (FLASH_START + FLASH_WINDOW) as u64 {
                warnings.push(format!(
                    "{} data is not (only) in flash ({:#010x}..{:#010x}); it can't be installed as firmware.",
                    f.name, f.min_addr, f.end_addr
                ));
            }
        }
        let kind = self.kind();
        match kind {
            ImageKind::Pinscape => {}
            ImageKind::PwmWorker => warnings.push(
                "This looks like the PWMWorker program (for a Pico on an expansion board), not Pinscape Pico firmware.".into(),
            ),
            ImageKind::Unknown => {
                warnings.push("This doesn't look like Pinscape Pico firmware (no Pinscape signature found).".into())
            }
        }
        Uf2Info { file_bytes: self.file_bytes, total_blocks: self.blocks.len() as u32, families, chips, kind, e10_block, warnings }
    }

    /// Identify the program by signature strings in the payload.
    pub fn kind(&self) -> ImageKind {
        let payload: Vec<u8> = self.blocks.iter().flat_map(|b| b.data.iter().copied()).collect();
        let has = |needle: &[u8]| payload.windows(needle.len()).any(|w| w == needle);
        if has(b"PWMWorker") {
            ImageKind::PwmWorker
        } else if has(b"Pinscape Pico firmware") || has(b"PinscapePico v") {
            ImageKind::Pinscape
        } else {
            ImageKind::Unknown
        }
    }

    /// The blocks a `chip` boot ROM would program (its families only, minus
    /// the E10 dummy block and non-flash blocks).
    pub fn blocks_for(&self, chip: Chip) -> impl Iterator<Item = &Block> {
        self.blocks.iter().filter(move |b| {
            b.is_flash_data() && !b.is_e10_workaround() && b.family.is_some_and(|f| chip.accepts(f))
        })
    }

    /// Build the erase/program plan for `chip`.
    pub fn plan(&self, chip: Chip) -> Result<FlashPlan, Uf2Error> {
        FlashPlan::build(self.blocks_for(chip), chip)
    }
}

/// Exactly what to do to flash: sectors to erase and page contents to write.
#[derive(Debug, Clone)]
pub struct FlashPlan {
    pub chip: Chip,
    /// 4 KB-aligned sector addresses, ascending.
    pub sectors: Vec<u32>,
    /// 256-byte pages (address → contents), ascending. Bytes in a page that
    /// the image doesn't cover are 0xFF (the erased state).
    pub pages: BTreeMap<u32, Vec<u8>>,
}

impl FlashPlan {
    pub fn build<'a>(blocks: impl IntoIterator<Item = &'a Block>, chip: Chip) -> Result<FlashPlan, Uf2Error> {
        // page address → (contents, coverage bitmap)
        let mut pages: BTreeMap<u32, (Vec<u8>, [bool; PAGE_SIZE as usize])> = BTreeMap::new();
        let mut any = false;
        for b in blocks {
            any = true;
            let len = b.data.len() as u32;
            if b.target_addr < FLASH_START || b.end_addr() > (FLASH_START + FLASH_WINDOW) as u64 {
                return Err(Uf2Error::OutsideFlash { addr: b.target_addr, len });
            }
            for (i, &byte) in b.data.iter().enumerate() {
                let addr = b.target_addr + i as u32;
                let page = addr & !(PAGE_SIZE - 1);
                let (data, covered) = pages.entry(page).or_insert_with(|| (vec![0xFF; PAGE_SIZE as usize], [false; PAGE_SIZE as usize]));
                let ofs = (addr - page) as usize;
                if covered[ofs] {
                    return Err(Uf2Error::Overlap { addr });
                }
                covered[ofs] = true;
                data[ofs] = byte;
            }
        }
        if !any {
            return Err(Uf2Error::NoBlocksForChip(chip.name()));
        }
        let sectors: BTreeSet<u32> = pages.keys().map(|p| p & !(SECTOR_SIZE - 1)).collect();
        for &s in &sectors {
            if PROTECTED_FLASH_SIZES.iter().any(|sz| s == FLASH_START + sz - SECTOR_SIZE) {
                return Err(Uf2Error::ProtectedSector { addr: s });
            }
        }
        Ok(FlashPlan {
            chip,
            sectors: sectors.into_iter().collect(),
            pages: pages.into_iter().map(|(a, (d, _))| (a, d)).collect(),
        })
    }

    /// Contiguous sector runs to erase, each at most `max_len` bytes
    /// (a multiple of the sector size).
    pub fn erase_ranges(&self, max_len: u32) -> Vec<(u32, u32)> {
        let max_len = (max_len / SECTOR_SIZE).max(1) * SECTOR_SIZE;
        let mut out: Vec<(u32, u32)> = Vec::new();
        for &s in &self.sectors {
            match out.last_mut() {
                Some((a, l)) if *a + *l == s && *l + SECTOR_SIZE <= max_len => *l += SECTOR_SIZE,
                _ => out.push((s, SECTOR_SIZE)),
            }
        }
        out
    }

    /// Contiguous page runs to write, each at most `max_len` bytes (a
    /// multiple of the page size) and never crossing a sector boundary.
    pub fn write_runs(&self, max_len: u32) -> Vec<(u32, Vec<u8>)> {
        let max_len = (max_len / PAGE_SIZE).max(1) * PAGE_SIZE;
        let mut out: Vec<(u32, Vec<u8>)> = Vec::new();
        for (&addr, data) in &self.pages {
            match out.last_mut() {
                Some((a, d))
                    if *a + d.len() as u32 == addr
                        && d.len() as u32 + PAGE_SIZE <= max_len
                        && addr % SECTOR_SIZE != 0 =>
                {
                    d.extend_from_slice(data)
                }
                _ => out.push((addr, data.clone())),
            }
        }
        out
    }

    pub fn total_write_bytes(&self) -> u64 {
        self.pages.len() as u64 * PAGE_SIZE as u64
    }

    /// Lowest address written and one past the highest.
    pub fn range(&self) -> (u32, u32) {
        let lo = self.sectors.first().copied().unwrap_or(FLASH_START);
        let hi = self.pages.keys().last().map(|p| p + PAGE_SIZE).unwrap_or(lo);
        (lo, hi)
    }
}

/// Build a UF2 file from (address, data) chunks, 256 bytes per block —
/// the layout the pico-sdk produces. Handy for tests and synthetic images.
pub fn build_image(family: u32, chunks: &[(u32, &[u8])]) -> Vec<u8> {
    let mut blocks = Vec::new();
    for &(addr, data) in chunks {
        for (i, c) in data.chunks(PAGE_SIZE as usize).enumerate() {
            blocks.push((addr + (i as u32) * PAGE_SIZE, c.to_vec()));
        }
    }
    let n = blocks.len() as u32;
    blocks
        .into_iter()
        .enumerate()
        .flat_map(|(i, (addr, data))| {
            Block { flags: 0, target_addr: addr, block_no: i as u32, num_blocks: n, family: Some(family), data }.encode()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pinscape_payload(len: usize) -> Vec<u8> {
        let mut v: Vec<u8> = (0..len).map(|i| (i * 7 + 3) as u8).collect();
        let sig = b"Pinscape Pico firmware v%d.%d.%d";
        v[100..100 + sig.len()].copy_from_slice(sig);
        v
    }

    #[test]
    fn block_roundtrip() {
        let b = Block { flags: flags::FAMILY_ID_PRESENT, target_addr: 0x1000_0100, block_no: 3, num_blocks: 9, family: Some(family::RP2040), data: vec![1, 2, 3] };
        let enc = b.encode();
        assert_eq!(&enc[0..4], b"UF2\n");
        assert_eq!(u32_at(&enc, 8), flags::FAMILY_ID_PRESENT);
        assert_eq!(Block::parse(&enc, 0).unwrap(), b);
    }

    #[test]
    fn rejects_malformed_files() {
        assert_eq!(Uf2Image::parse(&[]).unwrap_err(), Uf2Error::Empty);
        assert_eq!(Uf2Image::parse(&[0; 100]).unwrap_err(), Uf2Error::BadLength(100));
        assert_eq!(Uf2Image::parse(&[0; 512]).unwrap_err(), Uf2Error::BadMagic(0));

        let img = build_image(family::RP2040, &[(FLASH_START, &[0u8; 1024])]);
        // bad end magic in block 2
        let mut bad = img.clone();
        bad[2 * 512 + 511] ^= 1;
        assert_eq!(Uf2Image::parse(&bad).unwrap_err(), Uf2Error::BadMagic(2));
        // truncated: 3 of 4 blocks
        assert!(matches!(
            Uf2Image::parse(&img[..3 * 512]).unwrap_err(),
            Uf2Error::BlockCountMismatch { expected: 4, found: 3, .. }
        ));
        // duplicated block
        let mut dup = img[..3 * 512].to_vec();
        dup.extend_from_slice(&img[..512]);
        assert!(matches!(Uf2Image::parse(&dup).unwrap_err(), Uf2Error::DuplicateBlock { no: 0, .. }));
        // payload too large
        let mut big = img.clone();
        big[16..20].copy_from_slice(&477u32.to_le_bytes());
        assert!(matches!(Uf2Image::parse(&big).unwrap_err(), Uf2Error::BadPayloadSize { block: 0, size: 477 }));
        // block number >= count
        let mut num = img.clone();
        num[20..24].copy_from_slice(&4u32.to_le_bytes());
        assert!(matches!(Uf2Image::parse(&num).unwrap_err(), Uf2Error::BadBlockNumber { .. }));
    }

    #[test]
    fn info_reports_family_range_and_kind() {
        let data = pinscape_payload(3000);
        let img = Uf2Image::parse(&build_image(family::RP2040, &[(FLASH_START, &data)])).unwrap();
        let info = img.info();
        assert_eq!(info.total_blocks, 12);
        assert_eq!(info.file_bytes, 12 * 512);
        assert_eq!(info.chips, vec![Chip::Rp2040]);
        assert_eq!(info.kind, ImageKind::Pinscape);
        assert!(info.warnings.is_empty(), "{:?}", info.warnings);
        let f = &info.families[0];
        assert_eq!((f.id, f.blocks, f.payload_bytes, f.min_addr, f.end_addr), (family::RP2040, 12, 3000, FLASH_START, FLASH_START as u64 + 3000));

        let other = Uf2Image::parse(&build_image(family::RP2350_ARM_S, &[(FLASH_START, &[0u8; 256])])).unwrap();
        let info = other.info();
        assert_eq!(info.chips, vec![Chip::Rp2350]);
        assert_eq!(info.kind, ImageKind::Unknown);
        assert_eq!(info.warnings.len(), 1);
    }

    #[test]
    fn family_must_match_chip() {
        let img = Uf2Image::parse(&build_image(family::RP2040, &[(FLASH_START, &[0u8; 512])])).unwrap();
        assert!(img.plan(Chip::Rp2040).is_ok());
        assert_eq!(img.plan(Chip::Rp2350).unwrap_err(), Uf2Error::NoBlocksForChip("RP2350"));
        let img = Uf2Image::parse(&build_image(family::ABSOLUTE, &[(FLASH_START, &[0u8; 512])])).unwrap();
        assert!(img.plan(Chip::Rp2350).is_ok());
        assert_eq!(img.plan(Chip::Rp2040).unwrap_err(), Uf2Error::NoBlocksForChip("RP2040"));
        assert_eq!(Chip::from_cpu_type(2040), Some(Chip::Rp2040));
        assert_eq!(Chip::from_cpu_type(2350), Some(Chip::Rp2350));
        assert_eq!(Chip::from_cpu_type(1), None);
    }

    #[test]
    fn e10_block_is_skipped() {
        // like PWMWorker-pico2.uf2: an absolute-family dummy block, then an ARM-S image
        let mut file = Block {
            flags: flags::EXTENSION_TAGS_PRESENT,
            target_addr: E10_WORKAROUND_ADDR,
            block_no: 0,
            num_blocks: 2,
            family: Some(family::ABSOLUTE),
            data: vec![0xEF; 256],
        }
        .encode()
        .to_vec();
        file.extend(build_image(family::RP2350_ARM_S, &[(FLASH_START, &pinscape_payload(512))]));
        let img = Uf2Image::parse(&file).unwrap();
        let info = img.info();
        assert!(info.e10_block);
        assert_eq!(info.families.len(), 1);
        let plan = img.plan(Chip::Rp2350).unwrap();
        assert_eq!(plan.sectors, vec![FLASH_START]);
        assert_eq!(plan.pages.len(), 2);
    }

    #[test]
    fn plan_erases_only_covered_sectors_and_pads_pages() {
        // two separate regions: 0x10000000 (5000 bytes → 2 sectors), 0x10010080 (100 bytes, unaligned start)
        let a = pinscape_payload(5000);
        let b = [0xAAu8; 100];
        let img = Uf2Image::parse(&build_image(family::RP2040, &[(FLASH_START, &a), (FLASH_START + 0x10080, &b)])).unwrap();
        let plan = img.plan(Chip::Rp2040).unwrap();
        assert_eq!(plan.sectors, vec![FLASH_START, FLASH_START + 0x1000, FLASH_START + 0x10000]);
        assert_eq!(plan.pages.len(), 20 + 1);
        let last_a = &plan.pages[&(FLASH_START + 19 * 256)];
        assert_eq!(&last_a[..5000 - 19 * 256], &a[19 * 256..]);
        assert!(last_a[5000 - 19 * 256..].iter().all(|&x| x == 0xFF));
        let pb = &plan.pages[&(FLASH_START + 0x10000)];
        assert!(pb[..0x80].iter().all(|&x| x == 0xFF));
        assert_eq!(&pb[0x80..0x80 + 100], &b);

        assert_eq!(plan.erase_ranges(64 * 1024), vec![(FLASH_START, 0x2000), (FLASH_START + 0x10000, 0x1000)]);
        assert_eq!(plan.erase_ranges(4096), vec![(FLASH_START, 0x1000), (FLASH_START + 0x1000, 0x1000), (FLASH_START + 0x10000, 0x1000)]);
        let runs = plan.write_runs(4096);
        // 16 pages in sector 0, 4 in sector 1 (runs never cross a sector), 1 in the far sector
        assert_eq!(runs.iter().map(|(a, d)| (*a, d.len())).collect::<Vec<_>>(), vec![
            (FLASH_START, 4096),
            (FLASH_START + 0x1000, 1024),
            (FLASH_START + 0x10000, 256)
        ]);
        let runs = plan.write_runs(1024);
        assert_eq!(runs.len(), 4 + 1 + 1);
        assert_eq!(plan.total_write_bytes(), 21 * 256);
        assert_eq!(plan.range(), (FLASH_START, FLASH_START + 0x10100));
    }

    #[test]
    fn plan_refuses_protected_sectors_overlap_and_non_flash() {
        for size in PROTECTED_FLASH_SIZES {
            let addr = FLASH_START + size - 256;
            let img = Uf2Image::parse(&build_image(family::RP2040, &[(addr, &[0u8; 256])])).unwrap();
            assert_eq!(img.plan(Chip::Rp2040).unwrap_err(), Uf2Error::ProtectedSector { addr: FLASH_START + size - 4096 });
        }
        // just below the 2 MB top sector is fine
        let img = Uf2Image::parse(&build_image(family::RP2040, &[(FLASH_START + (2 << 20) - 8192, &[0u8; 256])])).unwrap();
        assert!(img.plan(Chip::Rp2040).is_ok());

        let img = Uf2Image::parse(&build_image(family::RP2040, &[(0x2000_0000, &[0u8; 256])])).unwrap();
        assert!(matches!(img.plan(Chip::Rp2040).unwrap_err(), Uf2Error::OutsideFlash { .. }));
        assert!(!img.info().warnings.is_empty());

        let img = Uf2Image::parse(&build_image(family::RP2040, &[(FLASH_START, &[0u8; 256]), (FLASH_START + 128, &[1u8; 256])])).unwrap();
        assert_eq!(img.plan(Chip::Rp2040).unwrap_err(), Uf2Error::Overlap { addr: FLASH_START + 128 });

        // NOT_MAIN_FLASH blocks are never planned
        let blk = Block { flags: flags::NOT_MAIN_FLASH, target_addr: FLASH_START, block_no: 0, num_blocks: 1, family: Some(family::RP2040), data: vec![0; 256] };
        let img = Uf2Image::parse(&blk.encode()).unwrap();
        assert_eq!(img.plan(Chip::Rp2040).unwrap_err(), Uf2Error::NoBlocksForChip("RP2040"));
    }

    /// The real v1.0.2 release images, when present on this machine.
    #[test]
    fn real_release_images_if_present() {
        let dir = "/tmp/claude-1000/-home-superhac-repos-testing-PinscapePico/49f48bcc-e2cb-4546-bb6b-37661bdf6ef2/scratchpad/release-v1.0.2/extracted";
        let cases = [
            ("PinscapePico-pico.uf2", Chip::Rp2040, ImageKind::Pinscape, false),
            ("PinscapePico-pico2.uf2", Chip::Rp2350, ImageKind::Pinscape, false),
            ("PWMWorker-pico.uf2", Chip::Rp2040, ImageKind::PwmWorker, false),
            ("PWMWorker-pico2.uf2", Chip::Rp2350, ImageKind::PwmWorker, true),
        ];
        for (name, chip, kind, e10) in cases {
            let Ok(bytes) = std::fs::read(format!("{dir}/{name}")) else { continue };
            let img = Uf2Image::parse(&bytes).unwrap();
            let info = img.info();
            assert_eq!(info.chips, vec![chip], "{name}");
            assert_eq!(info.kind, kind, "{name}");
            assert_eq!(info.e10_block, e10, "{name}");
            let plan = img.plan(chip).unwrap();
            assert_eq!(plan.sectors[0], FLASH_START, "{name}");
            assert!(plan.range().1 < FLASH_START + (2 << 20) - 4096, "{name}");
            let other = if chip == Chip::Rp2040 { Chip::Rp2350 } else { Chip::Rp2040 };
            assert!(img.plan(other).is_err(), "{name}");
        }
    }
}
