# Cellos Bare-Metal Installation Guide

## 1. Prerequisites
- USB-to-TTL serial adapter cable
- MicroSD card (Class 10, 8 GB – 16 GB recommended)
- MicroSD card reader
- BalenaEtcher flashing software
- PuTTY terminal emulator

## 2. Flashing the OS Image
- Insert the MicroSD card into the card reader and connect it to your computer.
- Launch BalenaEtcher.
- Click **Flash from file** and select your Cellos OS `.img` file.
- Click **Select target** and choose the MicroSD card drive. (Ensure you select the correct target drive to avoid overwriting your system disk).
- Click **Flash!** and wait for the process to complete with a "Success" notification.

> [!CAUTION]
> Once flashing is complete, Windows may display a prompt stating the disk is unreadable and asking to format it. Click **Cancel** immediately—do not format the drive. Safely remove the MicroSD card.

## 3. Hardware Setup
- Insert the flashed MicroSD card into the slot on the underside of the Raspberry Pi 3.
- Connect the three leads (Ground/Black, RX/White, TX/Green) of the USB-to-TTL cable to the Raspberry Pi GPIO header according to the serial pinout diagram. Do not connect the external power supply to the Raspberry Pi yet.
- Plug the USB end of the TTL adapter cable into your computer.

## 4. Connecting to the Serial Console via PuTTY
Before launching PuTTY, you must identify which COM port number Windows assigned to your USB-to-TTL adapter.
- On Windows, right-click the **Start** button and select **Device Manager**.
- Expand the **Ports (COM & LPT)** section. Locate your serial adapter (e.g., "Silicon Labs CP210x..." or "USB-SERIAL CH340..."). Note the assigned port identifier in parentheses (e.g., `COM3` or `COM4`).
- Launch PuTTY.
- Under **Connection type**, select **Serial**.
- In the **Serial line** field, enter the identified COM port (e.g., `COM3`).
- In the **Speed** field, enter `115200` (the standard default baud rate for the Raspberry Pi serial console).
- Click **Open**. A blank terminal window will appear. It remains blank because the Raspberry Pi is currently powered off.

## 5. Booting the System
- Connect the power supply to the Raspberry Pi to power on the board.
- Observe the PuTTY terminal window. If the OS image is valid, the kernel boot log (`dmesg`) will begin outputting to the screen.
- Once the boot sequence completes, the terminal will display a login prompt (e.g., `Cellos>`).