#!/bin/bash

mkdir -p /mnt/huge1G
(mount | grep /mnt/huge1G) > /dev/null || mount -t hugetlbfs hugetlbfs /mnt/huge1G
for i in {0..7}
do
	if [[ -e "/sys/devices/system/node/node$i" ]]
	then
		echo 1 > /sys/devices/system/node/node$i/hugepages/hugepages-1048576kB/nr_hugepages
	fi
done
