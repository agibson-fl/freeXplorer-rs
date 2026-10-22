//! Where an original Xbox keeps its partitions.
//!
//! Two layouts exist in the wild and they're told apart by whether the drive has a table:
//!
//! * with a table, the partitions are wherever it says, which is how a drive larger than the
//!   retail BIOS knew about gets an F that fills the disk, and how G appears at all;
//! * without one, the retail layout applies: X, Y, Z, C, E at fixed places, F capped at 120 GB
//!   and G taking whatever is left after it.
//!
//! Either way the five retail partitions are the same five, and their sizes never change.

use super::mbr::{SECTOR_SIZE, Table};
use crate::partition::{PartitionFileSystem, PartitionSpec};

/// The system partition, where the kernel and the dashboard live. Every formatted drive has
/// one, which is what makes it the best evidence that a table is really a table.
pub const SYSTEM_START: u32 = 0x465400;
/// The data partition, likewise.
pub const DATA_START: u32 = 0x55F400;

/// id, name, start sector, size in sectors. The sizes are not negotiable: the dashboard and
/// the kernel expect them, and XBPartitioner locks them for the same reason.
const CORE: [(&str, &str, u32, u32); 5] = [
    ("x", "Game Cache 1 (X)", 0x400, 0x177000),
    ("y", "Game Cache 2 (Y)", 0x177400, 0x177000),
    ("z", "Game Cache 3 (Z)", 0x2EE400, 0x177000),
    ("c", "System (C)", SYSTEM_START, 0xFA000),
    ("e", "Data (E)", DATA_START, 0x9896B0),
];

/// Whether this is one of the two partitions every formatted drive has.
pub fn is_essential_start(lba: u32) -> bool {
    lba == SYSTEM_START || lba == DATA_START
}

/// The first sector after E. Everything from here on is extended space.
pub const EXTENDED_START: u32 = 0xEE8AB0;

const EXTENDED_IDS: [&str; 4] = ["f", "g", "h", "i"];
const EXTENDED_NAMES: [&str; 4] = ["Extended (F)", "Extended (G)", "Extended (H)", "Extended (I)"];

/// A retail BIOS can only see 120 GB of F; on a bigger drive G picks up where that stops.
pub const F_CAP_WITHOUT_TABLE: u32 = 0xEA60000;

/// Where the console puts partitions: one of the five retail starts, or anywhere past E.
pub fn is_console_start(lba: u32) -> bool {
    CORE.iter().any(|(_, _, start, _)| *start == lba) || lba >= EXTENDED_START
}

/// Every partition the table describes, clamped to the drive it came off.
pub fn from_table(table: &Table, drive_len: u64) -> Vec<PartitionSpec> {
    let mut extended = 0;
    table
        .entries
        .iter()
        .map(
            |entry| match CORE.iter().find(|(_, _, start, _)| *start == entry.start_lba) {
                Some((id, name, _, _)) => spec(id, name, entry.offset(), entry.length(), drive_len),
                None => {
                    // more extended partitions than names: the last one claims the rest
                    let slot = extended.min(EXTENDED_IDS.len() - 1);
                    extended += 1;
                    spec(
                        EXTENDED_IDS[slot],
                        EXTENDED_NAMES[slot],
                        entry.offset(),
                        entry.length(),
                        drive_len,
                    )
                }
            },
        )
        .collect()
}

/// The layout a drive has when nothing wrote a table to it.
pub fn retail(drive_len: u64) -> Vec<PartitionSpec> {
    let mut specs: Vec<PartitionSpec> = CORE
        .iter()
        .map(|(id, name, start, size)| {
            spec(
                id,
                name,
                *start as u64 * SECTOR_SIZE,
                *size as u64 * SECTOR_SIZE,
                drive_len,
            )
        })
        .collect();

    let extended_start = EXTENDED_START as u64 * SECTOR_SIZE;
    if drive_len <= extended_start {
        return specs;
    }
    let remainder = drive_len - extended_start;
    let f = (F_CAP_WITHOUT_TABLE as u64 * SECTOR_SIZE).min(remainder);
    specs.push(spec("f", EXTENDED_NAMES[0], extended_start, f, drive_len));
    if remainder > f {
        specs.push(spec(
            "g",
            EXTENDED_NAMES[1],
            extended_start + f,
            remainder - f,
            drive_len,
        ));
    }
    specs
}

pub fn find<'a>(specs: &'a [PartitionSpec], name: &str) -> Option<&'a PartitionSpec> {
    let id = normalize(name);
    specs.iter().find(|s| s.id == id)
}

pub fn normalize(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

pub fn is_partition_id(name: &str) -> bool {
    let id = normalize(name);
    CORE.iter().any(|(core, _, _, _)| *core == id) || EXTENDED_IDS.contains(&id.as_str())
}

fn spec(id: &'static str, name: &'static str, offset: u64, length: u64, drive_len: u64) -> PartitionSpec {
    let length = if offset >= drive_len {
        0
    } else {
        length.min(drive_len - offset)
    };
    PartitionSpec {
        id,
        name,
        offset,
        length,
        file_system: PartitionFileSystem::Fatx,
        description: description(id),
    }
}

fn description(id: &str) -> &'static str {
    match id {
        "x" | "y" | "z" => "Game cache. The console reuses these freely, nothing in them lasts.",
        "c" => "The dashboard, and the files the kernel needs to boot.",
        "e" => "Saves, downloadable content and anything the dashboard installs.",
        _ => "Extended partition, holding the space the retail layout doesn't cover.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Table {
        Table::parse(&crate::og::mbr::tests::sector0()).unwrap()
    }

    #[test]
    fn a_table_drive_has_the_partitions_the_table_lists() {
        let specs = from_table(&table(), 250_000_000_000);
        let ids: Vec<&str> = specs.iter().map(|s| s.id).collect();
        assert_eq!(ids, ["x", "y", "z", "c", "e", "f"]);
        assert_eq!(specs[0].offset, 0x400 * 512);
    }

    #[test]
    fn the_system_partition_is_where_the_kernel_looks_for_it() {
        let specs = from_table(&table(), 250_000_000_000);
        let c = find(&specs, "C").expect("partition ids aren't case sensitive");
        assert_eq!(c.offset, 0x465400 * 512);
        assert_eq!(c.length, 0xFA000 * 512);
    }

    #[test]
    fn a_table_partition_past_the_end_of_the_drive_is_clamped() {
        let specs = from_table(&table(), 8_004_132_864);
        let f = find(&specs, "f").unwrap();
        assert_eq!(f.length, 0);
    }

    #[test]
    fn without_a_table_f_stops_at_120_gb_and_g_takes_the_rest() {
        let specs = retail(1_000_000_000_000);
        let f = find(&specs, "f").unwrap();
        let g = find(&specs, "g").unwrap();
        assert_eq!(f.offset, EXTENDED_START as u64 * 512);
        assert_eq!(f.length, F_CAP_WITHOUT_TABLE as u64 * 512);
        assert_eq!(g.offset, f.end());
        assert_eq!(g.end(), 1_000_000_000_000 / 512 * 512);
    }

    #[test]
    fn a_small_drive_without_a_table_has_no_g() {
        let specs = retail(10_000_000_000);
        assert!(find(&specs, "f").is_some());
        assert!(find(&specs, "g").is_none());
    }

    #[test]
    fn a_drive_too_small_for_the_retail_layout_still_lists_all_five() {
        // 2 GB doesn't even reach the start of E
        let specs = retail(2_000_000_000);
        let ids: Vec<&str> = specs.iter().map(|s| s.id).collect();
        assert_eq!(ids, ["x", "y", "z", "c", "e"]);
        assert_eq!(find(&specs, "e").unwrap().length, 0);
        assert!(find(&specs, "f").is_none());
    }

    #[test]
    fn only_the_starts_the_console_uses_are_recognised() {
        for (_, _, start, _) in CORE {
            assert!(is_console_start(start));
        }
        assert!(is_console_start(EXTENDED_START));
        assert!(is_console_start(0x7FFF_FFFF));
        assert!(!is_console_start(0x123456));
        assert!(!is_console_start(0));
    }
}
