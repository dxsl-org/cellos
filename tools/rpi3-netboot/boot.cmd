echo Cellos: starting static TFTP boot
usb start
setenv autoload no
setenv ipaddr 192.168.42.2
setenv serverip 192.168.42.1
setenv netmask 255.255.255.0
if tftpboot 0x01000000 cellos.uimg; then
    echo Cellos: TFTP complete, starting kernel
    bootm 0x01000000 - ${fdt_addr}
else
    echo Cellos: TFTP failed; check host server and cable
fi
