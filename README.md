# vroom

vroom is a userspace NVMe driver written in Rust.
It aims to be as fast as the SPDK NVMe driver, while minimizing unsafe code and offering a simplified API and less code.

# Build instructions

vroom needs to be compiled from source using the rust compiler.
Enable hugepages:

```bash
cd vroom
sudo ./setup-hugetlbfs.sh
```

To build the driver run:

```bash
cargo build --release --all-targets
```

# Using the IOMMU

By default, vroom needs root to directly access the NVMe device memory. By using the IOMMU and the Linux VFIO framework, the driver can be run without root privileges while also achieving improved safety.

1. Enable the IOMMU in the BIOS. On most Intel machines, the BIOS entry is called `VT-d` and has to be enabled in addition to any other virtualization technique.
2. Enable the IOMMU in the linux kernel. Add `intel_iommu=on` to your cmdline (if you are running a grub, the file `/etc/default/grub.cfg` contains a `GRUB_CMDLINE_LINUX` where you can add it).
3. Get the PCI address, vendor and device ID: `lspci -nn | grep NVM` returns something like `00:01.0 Non-Volatile memory controller [0108]: Red Hat, Inc. QEMU NVM Express Controller [1b36:0010] (rev 02)`. In this case, `0000:00:01.0` is our PCI Address, and `1b36` and `0010` are the vendor and device id, respectively.
4. Unbind the device from the Linux NVMe driver. `echo $PCI_ADDRESS > /sys/bus/pci/devices/$PCI_ADDRESS/driver/unbind`
5. Enable the `vfio-pci` driver. `modprobe vfio-pci`
6. Bind the device to the `vfio-pci` driver. `echo $VENDOR_ID $DEVICE_ID > /sys/bus/pci/drivers/vfio-pci/new_id`
7. Chown the device to the user. `chown $USER:$GROUP /dev/vfio/*`
8. That's it! Now you can compile and run vroom as stated above!
