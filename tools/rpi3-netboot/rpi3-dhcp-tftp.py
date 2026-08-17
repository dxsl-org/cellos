#!/usr/bin/env python3
"""Read-only TFTP server for Raspberry Pi 3 static-address U-Boot."""

from __future__ import annotations

import argparse
import logging
import socket
import struct
import threading
import time
from pathlib import Path

from protocols import is_allowed_client, parse_rrq, resolve_tftp_file


class NetbootServer:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.stop = threading.Event()

    def transfer(self, packet: bytes, client: tuple[str, int]) -> None:
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sock.bind((self.args.bind, 0))
        sock.settimeout(self.args.timeout)
        try:
            requested, options = parse_rrq(packet)
            path = resolve_tftp_file(self.args.root, requested)
            block_size = min(1468, max(512, int(options.get("blksize", "512"))))
            negotiated: list[bytes] = []
            if "blksize" in options:
                negotiated += [b"blksize", str(block_size).encode()]
            if "tsize" in options:
                negotiated += [b"tsize", str(path.stat().st_size).encode()]
            logging.info("TFTP RRQ %s -> %s (%d bytes)", requested, path.name, path.stat().st_size)
            if negotiated:
                self.send_wait(sock, b"\x00\x06" + b"\x00".join(negotiated) + b"\x00", client, 0)
            with path.open("rb") as source:
                block = 1
                while True:
                    data = source.read(block_size)
                    self.send_wait(sock, struct.pack("!HH", 3, block) + data, client, block)
                    if len(data) < block_size:
                        break
                    block = (block + 1) & 0xFFFF
            logging.info("TFTP DONE %s", requested)
        except (ValueError, FileNotFoundError) as error:
            message = str(error).encode("ascii", "replace")[:120]
            sock.sendto(struct.pack("!HH", 5, 1) + message + b"\x00", client)
            logging.warning("TFTP ERROR %s", error)
        except (TimeoutError, OSError) as error:
            logging.error("TFTP FAILED %s: %s", client, error)
        finally:
            sock.close()

    def send_wait(self, sock: socket.socket, data: bytes, client: tuple[str, int], block: int) -> None:
        for _ in range(5):
            sock.sendto(data, client)
            try:
                reply, sender = sock.recvfrom(2048)
            except TimeoutError:
                continue
            if sender == client and reply[:4] == struct.pack("!HH", 4, block):
                return
        raise TimeoutError(f"no ACK for block {block}")

    def tftp_loop(self, sock: socket.socket) -> None:
        logging.info(
            "TFTP listening on %s:%d client=%s root=%s",
            self.args.bind,
            self.args.tftp_port,
            self.args.client,
            self.args.root,
        )
        while not self.stop.is_set():
            try:
                packet, client = sock.recvfrom(4096)
            except TimeoutError:
                continue
            if not is_allowed_client(client, self.args.client):
                logging.warning("TFTP REJECT %s:%d", *client)
                continue
            threading.Thread(target=self.transfer, args=(packet, client), daemon=True).start()
        sock.close()

    def run(self) -> None:
        deadline = time.monotonic() + self.args.bind_wait
        waiting_logged = False
        while True:
            tftp_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            try:
                tftp_sock.settimeout(1)
                tftp_sock.bind((self.args.bind, self.args.tftp_port))
                break
            except OSError as error:
                tftp_sock.close()
                unavailable = error.errno in (99, 10049) or getattr(error, "winerror", None) == 10049
                if not unavailable or time.monotonic() >= deadline:
                    raise
                if not waiting_logged:
                    logging.info("Waiting for bind address %s", self.args.bind)
                    waiting_logged = True
                time.sleep(0.25)
        thread = threading.Thread(target=self.tftp_loop, args=(tftp_sock,))
        thread.start()
        try:
            thread.join()
        except KeyboardInterrupt:
            self.stop.set()
            thread.join(2)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bind", required=True)
    parser.add_argument("--client", required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--log", type=Path, required=True)
    parser.add_argument("--tftp-port", type=int, default=69)
    parser.add_argument("--bind-wait", type=float, default=0)
    parser.add_argument("--timeout", type=float, default=2.0)
    args = parser.parse_args()
    args.root = args.root.resolve(strict=True)
    args.log.parent.mkdir(parents=True, exist_ok=True)
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s", handlers=[logging.FileHandler(args.log), logging.StreamHandler()])
    NetbootServer(args).run()


if __name__ == "__main__":
    main()
