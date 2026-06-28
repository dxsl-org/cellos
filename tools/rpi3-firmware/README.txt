Raspberry Pi 3 VideoCore firmware files (not included in repo)
==============================================================

Download the following files from the Raspberry Pi Foundation firmware repo:
(free to redistribute; binary-only)

  https://github.com/raspberrypi/firmware/raw/master/boot/bootcode.bin
  https://github.com/raspberrypi/firmware/raw/master/boot/start.elf
  https://github.com/raspberrypi/firmware/raw/master/boot/fixup.dat

PowerShell download (run from project root):

  $fw = "https://github.com/raspberrypi/firmware/raw/master/boot"
  Invoke-WebRequest "$fw/bootcode.bin" -OutFile "tools/rpi3-firmware/bootcode.bin"
  Invoke-WebRequest "$fw/start.elf"   -OutFile "tools/rpi3-firmware/start.elf"
  Invoke-WebRequest "$fw/fixup.dat"   -OutFile "tools/rpi3-firmware/fixup.dat"

After downloading, run:
  .\gen_disk_rpi3.ps1

config.txt is already present in this directory.
