#!/usr/bin/env python3
"""Bridge a VMware Workstation named-pipe serial port to stdio (Windows `py -3`).

Usage: py -3 vmware_serial_relay.py \\\\.\\pipe\\cellos-<run> [--connect-timeout S]

VMware owns the pipe (`serial0.fileType = "pipe"`, `serial0.pipe.endPoint =
"server"`); this relay is its one client. The WSL harness
(`test-native-vmware-matrix.py`) starts it before `vmrun start` and talks to it
over stdio:

- stdout: every byte the guest UART sent, unmodified, flushed per chunk;
- stdin:  bytes to type into the guest UART, written in arrival order;
- stderr: one line per event: `relay: connected <pipe>`, `relay: closed <why>`.

Exit status: 0 when the VM closes the pipe (power-off), 2 on a usage error,
3 when the pipe never appears within the connect timeout, 4 on any other pipe
error. Standard library only; no VMware Tools in the guest.

The pipe handle is synchronous, and Windows serialises synchronous I/O on one
handle: a blocked ReadFile would hold back every WriteFile. The reader therefore
never blocks in ReadFile; it asks PeekNamedPipe how many bytes are waiting and
reads exactly those, so a write from the stdin thread always gets through.
"""

import argparse
import ctypes
from ctypes import wintypes
import os
import re
import sys
import threading
import time

GENERIC_READ = 0x80000000
GENERIC_WRITE = 0x40000000
OPEN_EXISTING = 3
INVALID_HANDLE_VALUE = wintypes.HANDLE(-1).value
ERROR_FILE_NOT_FOUND = 2
ERROR_BROKEN_PIPE = 109
ERROR_PIPE_BUSY = 231
ERROR_NO_DATA = 232
ERROR_PIPE_NOT_CONNECTED = 233
CLOSED = (ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_NOT_CONNECTED)
PIPE_NAME = re.compile(r"\\\\\.\\pipe\\cellos-[A-Za-z0-9._-]{1,64}")
POLL_SECONDS = 0.005


class PipeClosed(Exception):
    pass


def kernel32():
    lib = ctypes.WinDLL("kernel32", use_last_error=True)
    lib.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, wintypes.LPVOID,
                                wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    lib.CreateFileW.restype = wintypes.HANDLE
    lib.WaitNamedPipeW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD]
    lib.WaitNamedPipeW.restype = wintypes.BOOL
    lib.PeekNamedPipe.argtypes = [wintypes.HANDLE, wintypes.LPVOID, wintypes.DWORD,
                                  wintypes.LPDWORD, wintypes.LPDWORD, wintypes.LPDWORD]
    lib.PeekNamedPipe.restype = wintypes.BOOL
    lib.ReadFile.argtypes = [wintypes.HANDLE, wintypes.LPVOID, wintypes.DWORD,
                             wintypes.LPDWORD, wintypes.LPVOID]
    lib.ReadFile.restype = wintypes.BOOL
    lib.WriteFile.argtypes = [wintypes.HANDLE, wintypes.LPCVOID, wintypes.DWORD,
                              wintypes.LPDWORD, wintypes.LPVOID]
    lib.WriteFile.restype = wintypes.BOOL
    lib.CloseHandle.argtypes = [wintypes.HANDLE]
    lib.CloseHandle.restype = wintypes.BOOL
    return lib


def fail(call):
    code = ctypes.get_last_error()
    if code in CLOSED:
        raise PipeClosed(call + " error " + str(code))
    raise OSError(call + " failed with Windows error " + str(code))


class Pipe:
    def __init__(self, k32, name, timeout):
        self.k32 = k32
        self.lock = threading.Lock()
        deadline = time.monotonic() + timeout
        while True:
            handle = k32.CreateFileW(name, GENERIC_READ | GENERIC_WRITE, 0, None,
                                     OPEN_EXISTING, 0, None)
            if handle != INVALID_HANDLE_VALUE:
                self.handle = handle
                return
            code = ctypes.get_last_error()
            if time.monotonic() >= deadline:
                raise TimeoutError("pipe not available (last Windows error " + str(code) + ")")
            if code == ERROR_PIPE_BUSY:
                k32.WaitNamedPipeW(name, 1000)
            elif code == ERROR_FILE_NOT_FOUND:
                time.sleep(0.05)
            else:
                raise OSError("CreateFileW failed with Windows error " + str(code))

    def available(self):
        count = wintypes.DWORD(0)
        with self.lock:
            if not self.k32.PeekNamedPipe(self.handle, None, 0, None, ctypes.byref(count), None):
                fail("PeekNamedPipe")
        return count.value

    def read(self, size):
        buffer = ctypes.create_string_buffer(size)
        done = wintypes.DWORD(0)
        with self.lock:
            if not self.k32.ReadFile(self.handle, buffer, size, ctypes.byref(done), None):
                fail("ReadFile")
        return buffer.raw[:done.value]

    def write(self, data):
        view = memoryview(data)
        while view:
            done = wintypes.DWORD(0)
            with self.lock:
                if not self.k32.WriteFile(self.handle, bytes(view), len(view), ctypes.byref(done), None):
                    fail("WriteFile")
            view = view[done.value:]

    def close(self):
        self.k32.CloseHandle(self.handle)


def type_stdin(pipe, closed):
    """Copy stdin to the guest until stdin ends or the pipe closes."""
    stream = sys.stdin.buffer
    try:
        while not closed.is_set():
            chunk = stream.read1(4096)
            if not chunk:
                return
            pipe.write(chunk)
    except (PipeClosed, OSError):
        closed.set()


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("pipe", help=r"\\.\pipe\cellos-<run>")
    parser.add_argument("--connect-timeout", type=float, default=60.0,
                        help="seconds to wait for VMware to create the pipe (default: 60)")
    args = parser.parse_args()
    if sys.platform != "win32":
        print("relay: closed not-windows", file=sys.stderr, flush=True)
        return 2
    if not PIPE_NAME.fullmatch(args.pipe):
        print("relay: closed bad-pipe-name " + args.pipe, file=sys.stderr, flush=True)
        return 2
    try:
        pipe = Pipe(kernel32(), args.pipe, args.connect_timeout)
    except TimeoutError as exc:
        print("relay: closed " + str(exc), file=sys.stderr, flush=True)
        return 3
    except OSError as exc:
        print("relay: closed " + str(exc), file=sys.stderr, flush=True)
        return 4
    print("relay: connected " + args.pipe, file=sys.stderr, flush=True)
    closed = threading.Event()
    threading.Thread(target=type_stdin, args=(pipe, closed), daemon=True).start()
    out = sys.stdout.buffer
    status, reason = 0, "vm-closed-pipe"
    try:
        while not closed.is_set():
            count = pipe.available()
            if count:
                out.write(pipe.read(count))
                out.flush()
            else:
                time.sleep(POLL_SECONDS)
    except PipeClosed:
        pass
    except OSError as exc:
        status, reason = 4, str(exc)
    finally:
        closed.set()
        pipe.close()
    print("relay: closed " + reason, file=sys.stderr, flush=True)
    return status


if __name__ == "__main__":
    code = main()
    sys.stdout.flush()
    sys.stderr.flush()
    # The stdin thread may still sit in a blocking read the harness never ends;
    # a normal interpreter exit would abort on its buffer lock, so leave at once.
    os._exit(code)
