#!/bin/bash

if [ -n "$1" ]; then
    nr_hugepages="$1"
else
    nr_hugepages=8
fi

mkdir -p /mnt/huge
(mount | grep /mnt/huge) > /dev/null || mount -t hugetlbfs hugetlbfs /mnt/huge
for i in {0..7}
do
    if [[ -e "/sys/devices/system/node/node$i" ]]
    then
        echo $nr_hugepages > /sys/devices/system/node/node$i/hugepages/hugepages-2048kB/nr_hugepages
    fi
done
