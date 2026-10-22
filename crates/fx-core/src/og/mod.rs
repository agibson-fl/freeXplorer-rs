//! Original Xbox drives.
//!
//! A different machine with a different layout from the 360: five fixed partitions the console
//! and the dashboard both expect, then whatever extended space the drive's table describes.
//! Unlike the 360 layout, this one is read off the drive rather than compiled in, because it is
//! the drive's own table that says how the space past E was divided.

pub mod layout;
pub mod mbr;
pub mod scanner;

pub use layout::{find, from_table, retail};
pub use mbr::{Entry, Table};
pub use scanner::scan;
