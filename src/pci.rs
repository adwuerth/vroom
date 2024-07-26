use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};

use byteorder::{NativeEndian, ReadBytesExt, WriteBytesExt};

use crate::{Error, Result};

// PCI utility functions

// write to the command register (offset 4) in the PCIe config space
pub const COMMAND_REGISTER_OFFSET: u64 = 4;
// bit 2: "bus master enable", see PCIe 3.0 specification section 7.5.1.1
pub const BUS_MASTER_ENABLE_BIT: u64 = 2;
// bit 10: "interrupt disable"
pub const INTERRUPT_DISABLE: u64 = 10;

/// Opens a pci resource file at the given address.
pub fn pci_open_resource(pci_addr: &str, resource: &str) -> Result<File> {
    let path = format!("/sys/bus/pci/devices/{pci_addr}/{resource}");
    Ok(OpenOptions::new().read(true).write(true).open(path)?)
}

/// Opens a pci resource file at the given address in read-only mode.
pub fn pci_open_resource_ro(pci_addr: &str, resource: &str) -> Result<File> {
    let path = format!("/sys/bus/pci/devices/{pci_addr}/{resource}");
    Ok(OpenOptions::new().read(true).write(false).open(path)?)
}

/// Reads and returns an u8 at `offset` in `file`.
pub fn read_io8(file: &mut File, offset: u64) -> Result<u8> {
    file.seek(SeekFrom::Start(offset))?;
    let res = file.read_u8()?;
    Ok(res)
}

/// Reads and returns an u16 at `offset` in `file`.
pub fn read_io16(file: &mut File, offset: u64) -> Result<u16> {
    file.seek(SeekFrom::Start(offset))?;
    let res = file.read_u16::<NativeEndian>()?;
    Ok(res)
}

/// Reads and returns an u32 at `offset` in `file`.
pub fn read_io32(file: &mut File, offset: u64) -> Result<u32> {
    file.seek(SeekFrom::Start(offset))?;
    let res = file.read_u32::<NativeEndian>()?;
    Ok(res)
}

/// Writes an u64 at `offset` in `file`.
pub fn read_io64(file: &mut File, offset: u64) -> Result<u64> {
    file.seek(SeekFrom::Start(offset))?;
    let res = file.read_u64::<NativeEndian>()?;
    Ok(res)
}

/// Writes an u8 at `offset` in `file`.
pub fn write_io8(file: &mut File, value: u8, offset: u64) -> Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    let res = file.write_u8(value)?;
    Ok(res)
}

/// Writes an u16 at `offset` in `file`.
pub fn write_io16(file: &mut File, value: u16, offset: u64) -> Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    let res = file.write_u16::<NativeEndian>(value)?;
    Ok(res)
}

/// Writes an u32 at `offset` in `file`.
pub fn write_io32(file: &mut File, value: u32, offset: u64) -> Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    let res = file.write_u32::<NativeEndian>(value)?;
    Ok(res)
}

/// Writes an u64 at `offset` in `file`.
pub fn write_io64(file: &mut File, value: u64, offset: u64) -> Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    let res = file.write_u64::<NativeEndian>(value)?;
    Ok(res)
}

/// Reads a hex string from `file` and returns it as `u64`.
pub fn read_hex(file: &mut File) -> Result<u64> {
    let mut buffer = String::new();
    file.read_to_string(&mut buffer)?;

    u64::from_str_radix(buffer.trim().trim_start_matches("0x"), 16)
        .map_err(|error| Error::Custom(error.to_string()))
}
