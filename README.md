# vroom

vroom is a userspace NVMe driver written in Rust.
As an userspace driver which (optionally) uses VFIO, it can be run without root privileges.
It aims to be as fast as the SPDK NVMe driver, while minimizing unsafe code and offering a simplified API.
vroom currently serves as a proof of concept and has many features yet to be implemented.

For further details take a look at [@bootreer](https://github.com/bootreer)'s [thesis](https://db.in.tum.de/people/sites/ellmann/theses/finished/24/pirhonen_writing_an_nvme_driver_in_rust.pdf) on vroom


# Build instructions

vroom needs to be compiled from source using rust's package manager `cargo`.
Vroom uses hugepages, enable them using:

```bash
cd vroom
sudo ./scripts/setup-hugetlbfs.sh
```

To build the driver run:

```bash
cargo build --release --all-targets
```

An example can be run by using:

```bash
cargo run --example <example>
```

To re-bind the kernel driver after vroom use

```bash
./scripts/bind-kernel-driver.sh <pci_address>
```

# Using the IOMMU

By default, vroom needs root to directly access the NVMe device memory. By using the IOMMU and the Linux VFIO framework, the driver can be run without root privileges while also achieving improved safety.

1. Enable the IOMMU in the BIOS. On most Intel machines, the BIOS entry is called `VT-d` and has to be enabled in addition to any other virtualization technique.
2. Enable the IOMMU in the linux kernel. Add `intel_iommu=on` to your cmdline (if you are running a grub, the file `/etc/default/grub.cfg` contains a `GRUB_CMDLINE_LINUX` where you can add it).

From step 3 you can either use a provided script or continue manually.
To bind the vfio driver using the script execute

```bash
./scripts/bind-vfio-driver.sh <pci_address> <user> <group>
```

To unbind the vfio driver use

```bash
./scripts/unbind-vfio-driver.sh <pci_address>
```

To enable it manually:

3. Get the PCI address, vendor and device ID: `lspci -nn | grep NVM` returns something like `00:01.0 Non-Volatile memory controller [0108]: Red Hat, Inc. QEMU NVM Express Controller [1b36:0010] (rev 02)`. In this case, `0000:00:01.0` is our PCI Address, and `1b36` and `0010` are the vendor and device id, respectively.
4. Unbind the device from the Linux NVMe driver. `echo $PCI_ADDRESS > /sys/bus/pci/devices/$PCI_ADDRESS/driver/unbind`
5. Enable the `vfio-pci` driver. `modprobe vfio-pci`
6. Bind the device to the `vfio-pci` driver. `echo $VENDOR_ID $DEVICE_ID > /sys/bus/pci/drivers/vfio-pci/new_id`
7. Chown the device to the user. `chown $USER:$GROUP /dev/vfio/*`
8. That's it! Now you can compile and run vroom as stated above!

# Testing

Currently there are a few integration tests implemented. It is necessary to first set an environment variable containing the NVMe PCI address, for example for `0000:00:01.0`:

```bash
export NVME_ADDR="0000:00:01.0"
```

Then, run the tests using:

```bash
cargo test
```