#!/bin/bash

if [ -n "$1" ]; then
  nr_hugepages="$1"
else
  echo "Error: No hugepage count provided."
  exit 1
fi

mkdir -p /mnt/huge
(mount | grep /mnt/huge) > /dev/null || mount -t hugetlbfs hugetlbfs /mnt/huge
for i in {0..7}
do
	if [[ -e "/sys/devices/system/node/node$i" ]]
	then
		echo $nr_hugepages > /sys/devices/system/node/node$i/hugepages/hugepages-1048576kB/nr_hugepages
	fi
done
