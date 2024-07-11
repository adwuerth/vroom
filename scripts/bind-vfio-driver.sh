#!/bin/bash

if [ -n "$nvme_address" ]; then
  nvme="$nvme_address"
elif [ -n "$1" ]; then
  nvme="$1"
  shift
else
  echo "Error: No PCI address provided. Set the nvme_address environment variable or provide a PCI address as an argument."
  exit 1
fi

if [ -n "$1" ]; then
  user="$1"
else
  echo "Error: No user provided. Please provide a user as the second argument."
  exit 1
fi

if [ -n "$2" ]; then
  group="$2"
else
  echo "Error: No group provided. Please provide a group as the third argument."
  exit 1
fi

modprobe vfio-pci
nvme_vd="$(cat /sys/bus/pci/devices/$nvme/vendor) $(cat /sys/bus/pci/devices/$nvme/device)"
# echo "$nvme_vd" > "/sys/bus/pci/drivers/vfio-pci/remove_id"
# echo 1 > "/sys/bus/pci/devices/$nvme/remove"
# echo 1 > "/sys/bus/pci/rescan"
echo $nvme > /sys/bus/pci/devices/$nvme/driver/unbind
echo "$nvme_vd" > /sys/bus/pci/drivers/vfio-pci/new_id
chown $user:$group /dev/vfio/*
