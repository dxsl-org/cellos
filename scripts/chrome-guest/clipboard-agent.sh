#!/bin/sh
# Chrome guest clipboard agent (/usr/lib/chrome-guest/clipboard-agent), run as
# the chrome user for the guest's whole life.
#
# The CellOS clipboard reaches the guest on the virtio console port $1 (the
# hypervisor's line protocol, both ways): one line per text, the byte "C", the
# base64 of the UTF-8 text, a newline.
#
# wl-paste and wl-copy speak wlr-data-control (cage gets it from
# data-control.so), so the agent needs no window of its own. Copying here
# triggers the watcher, which hands the same text back; the host drops text
# equal to what it holds, so nothing loops. What the agent moves (byte counts,
# never text) and its errors go to $2, which the guest init prints when cage
# ends.
port=$1
log=$2
export WAYLAND_DISPLAY=wayland-0

# The port stays open on fd 3 for the agent's life: hvc resets a port's
# terminal settings at its last close, and these must hold. Raw: no CR added
# to each newline, which would end every line in a byte base64 refuses. No
# echo: the guest would send every line it receives straight back.
exec 3<>"$port" || { echo "cannot open $port" >> "$log"; exit 1; }
stty -F "$port" raw -echo 2>> "$log" || echo "stty $port failed" >> "$log"

# Guest -> host. The watcher ends with cage (Chromium's restart starts a new
# cage), so it is started again whenever cage's socket is there.
(
    while :; do
        if [ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]; then
            wl-paste --no-newline --type text --watch sh -c \
                'text=$(head -c 65536 | base64 -w 0); printf "C%s\n" "$text" > "$1"
                 echo "to-host ${#text} base64 bytes" >> "$0"' "$log" "$port" 2>> "$log"
        fi
        sleep 1
    done
) &

# Host -> guest.
while IFS= read -r line <&3; do
    case "$line" in
    C?*)
        printf %s "${line#C}" | base64 -d | wl-copy 2>> "$log"
        echo "from-host $((${#line} - 1)) base64 bytes, wl-copy $?" >> "$log"
        ;;
    esac
done
echo "port closed" >> "$log"
