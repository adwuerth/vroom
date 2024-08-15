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

# if [ -n "$1" ]; then
#   user="$1"
# else
#   echo "Error: No user provided. Please provide a user as the second argument."
#   exit 1
# fi

# if [ -n "$2" ]; then
#   group="$2"
# else
#   echo "Error: No group provided. Please provide a group as the third argument."
#   exit 1
# fi

function vfio_bind {
    nvme=$1
    dpath="/sys/bus/pci/devices/$nvme"

    echo "Binding: $nvme"
    echo "vfio-pci" > "$dpath/driver_override"

    if [[ -d $dpath ]]; then
        curr_driver=$(readlink $dpath/driver)
        curr_driver=${curr_driver##*/}

        if [[ $curr_driver == "vfio-pci" ]]; then
            echo "$nvme already bound to vfio-pci" 1>&2
            continue
        else
            echo $nvme > "$dpath/driver/unbind"
            echo "Unbound $nvme from $curr_driver" 1>&2
        fi
    fi

    echo $nvme > /sys/bus/pci/drivers_probe
}

modprobe vfio-pci
# nvme_vd="$(cat /sys/bus/pci/devices/$nvme/vendor) $(cat /sys/bus/pci/devices/$nvme/device)"
# echo $nvme > /sys/bus/pci/devices/$nvme/driver/unbind
# echo "$nvme_vd" > /sys/bus/pci/drivers/vfio-pci/new_id
# chown $user:$group /dev/vfio/*

# echo "vfio-pci" > "/sys/bus/pci/devices/$nvme/driver_override"
# echo $nvme > /sys/bus/pci/drivers_probe

vfio_bind $nvme