//! Working out whether a drive is an original Xbox drive.
//!
//! Two things can make it one: a partition table in the first sector, or FATX volumes sitting
//! at the retail offsets. The first is how a drive bigger than the retail BIOS understood gets
//! its space back; the second is a drive nobody ever gave a table to.

use crate::device::BlockDevice;
use crate::error::Result;
use crate::partition::PartitionSpec;
use crate::scan::{DetectedPartition, DriveKind, DriveScan};

use super::{layout, mbr};

pub fn scan(device: &dyn BlockDevice) -> Result<Option<DriveScan>> {
    let table = mbr::read(device)?;
    let (specs, has_table): (Vec<PartitionSpec>, bool) = match &table {
        Some(table) => (layout::from_table(table, device.len()), true),
        None => (layout::retail(device.len()), false),
    };

    let mut partitions = Vec::with_capacity(specs.len());
    let mut any_fatx = false;
    for spec in specs {
        if spec.length == 0 || spec.offset >= device.len() {
            partitions.push(DetectedPartition {
                spec,
                has_signature: false,
                flavor: None,
            });
            continue;
        }
        let flavor = crate::scan::signature_at(device, spec.offset)?;
        any_fatx |= flavor.is_some();
        partitions.push(DetectedPartition {
            spec,
            has_signature: flavor.is_some(),
            flavor,
        });
    }

    // A table is evidence on its own: something wrote a console partition table here. The
    // retail layout is guesswork, so it only counts if a FATX volume is really where it says.
    if !has_table && !any_fatx {
        return Ok(None);
    }

    Ok(Some(DriveScan {
        kind: DriveKind::OgDrive,
        visible_length: device.len() / mbr::SECTOR_SIZE * mbr::SECTOR_SIZE,
        has_security_sector: false,
        partitions,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::MemoryDevice;

    /// The real table, on a drive of the length it came off.
    fn drive_with_table() -> MemoryDevice {
        let device = MemoryDevice::new(250_000_000_000);
        device.write_at(0, &crate::og::mbr::tests::sector0()).unwrap();
        device
    }

    fn write_signature(device: &MemoryDevice, at: u64) {
        device.write_at(at, b"FATX").unwrap();
    }

    #[test]
    fn a_table_is_enough_on_its_own() {
        let scan = scan(&drive_with_table()).unwrap().expect("a drive with a table");
        assert_eq!(scan.kind, DriveKind::OgDrive);
        assert_eq!(scan.partitions.len(), 6);
        assert!(!scan.has_security_sector);
    }

    #[test]
    fn the_partitions_the_table_lists_are_all_there() {
        let scan = scan(&drive_with_table()).unwrap().unwrap();
        let ids: Vec<&str> = scan.partitions.iter().map(|p| p.id()).collect();
        assert_eq!(ids, ["x", "y", "z", "c", "e", "f"]);
    }

    #[test]
    fn a_fatx_volume_somewhere_the_retail_layout_expects_is_an_xbox_drive() {
        let device = MemoryDevice::new(250_000_000_000);
        device.write_at(0, &[0u8; 512]).unwrap();
        let specs = layout::retail(device.len());
        let c = layout::find(&specs, "c").unwrap();
        write_signature(&device, c.offset);

        let scan = scan(&device).unwrap().expect("it's an original Xbox drive");
        assert_eq!(scan.kind, DriveKind::OgDrive);
        let c = scan.partitions.iter().find(|p| p.id() == "c").unwrap();
        assert!(c.can_mount());
        assert_eq!(c.flavor, Some(crate::fatx::Flavor::Xbox));
    }

    #[test]
    fn a_blank_drive_is_not_an_xbox_drive() {
        let device = MemoryDevice::new(250_000_000_000);
        assert!(scan(&device).unwrap().is_none());
    }

    #[test]
    fn a_volume_without_a_fatx_signature_doesnt_count() {
        let device = MemoryDevice::new(250_000_000_000);
        let specs = layout::retail(device.len());
        let c = layout::find(&specs, "c").unwrap();
        device.write_at(c.offset, b"NOPE").unwrap();
        assert!(scan(&device).unwrap().is_none());
    }

    /// A 360 volume sitting where the retail layout expects C is still an original Xbox drive:
    /// the drive is laid out like one, only its file system was written by the other console.
    #[test]
    fn the_layout_decides_the_console_not_the_volume_flavor() {
        let device = MemoryDevice::new(250_000_000_000);
        let specs = layout::retail(device.len());
        let c = layout::find(&specs, "c").unwrap();
        device.write_at(c.offset, b"XTAF").unwrap();

        let scan = scan(&device).unwrap().expect("the layout says original Xbox");
        let c = scan.partitions.iter().find(|p| p.id() == "c").unwrap();
        assert_eq!(c.flavor, Some(crate::fatx::Flavor::Xbox360));
    }
}
