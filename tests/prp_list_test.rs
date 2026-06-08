mod common;
use common::*;

#[test]
pub fn prp_list_roundtrip() {
    let pci_addr = &get_pci_addr();
    let mut nvme = init_nvme(pci_addr);

    // 256 KiB == 64 NVMe pages == 63 PRP-list entries in a single command
    let len = 256 * 1024;
    let lba = 0;

    let pattern: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();

    nvme.write_copied(&pattern, lba)
        .expect("write_copied failed");

    let mut read_back = vec![0u8; len];
    nvme.read_copied(&mut read_back, lba)
        .expect("read_copied failed");

    assert!(
        pattern == read_back,
        "data read back over the PRP-list path does not match what was written"
    );
}
