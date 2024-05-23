#!/bin/bash

if [ -n "$nvme_address" ]; then
  nvme="$nvme_address"
elif [ -n "$1" ]; then
  nvme="$1"
else
  echo "Error: No PCI address provided. Set the nvme_address environment variable or provide a PCI address as an argument."
  exit 1
fi

echo $nvme > /sys/bus/pci/drivers/nvme/bind
