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

echo "$nvme_vd" > "/sys/bus/pci/drivers/vfio-pci/remove_id"
# todo try autoprobe
# echo 1 > "/sys/bus/pci/devices/$nvme/remove"
# echo 1 > "/sys/bus/pci/rescan"

