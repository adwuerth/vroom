use vroom::memory::Dma;
use vroom::PAGESIZE_2MIB;

mod common;
use common::*;

#[test]
pub fn simple_read_write() {
    let pci_addr = &get_pci_addr();

    let lba = 0;

    let mut nvme = init_nvme(pci_addr);

    let bytes: &[u8] = b"hello world! vroom test bytes";
    let mut buffer: Dma<u8> = allocate_dma_buffer(&nvme, PAGESIZE_2MIB);

    buffer[..bytes.len()].copy_from_slice(bytes);
    nvme_write(&mut nvme, &buffer, lba);

    buffer[..bytes.len()].fill_with(Default::default);
    nvme_read(&mut nvme, &mut buffer, lba);

    let read_buf = &buffer[0..bytes.len()];
    assert_eq!(
        bytes, read_buf,
        "Data read from NVMe does not match expected data"
    );
}
