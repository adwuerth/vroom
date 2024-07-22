#!/bin/bash

if [ -n "$nvme_address" ]; then
  nvme="$nvme_address"
elif [ -n "$1" ]; then
  nvme="$1"
else
  echo "Error: No PCI address provided. Set the nvme_address environment variable or provide a PCI address as an argument."
  exit 1
fi

nvme_vd="$(cat /sys/bus/pci/devices/$nvme/vendor) $(cat /sys/bus/pci/devices/$nvme/device)"
modprobe pci-stub
echo $nvme > /sys/bus/pci/devices/$nvme/driver/unbind
# echo "$nvme_vd" > /sys/bus/pci/drivers/pci-stub/new_id
echo $nvme > /sys/bus/pci/drivers/pci-stub/bind