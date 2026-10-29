"""Restricted, stdlib-only, all-stop QEMU RSP observer (AF_UNIX only).

API: connect() negotiates qSupported, queries stop state, and loads target XML
(including xi:include); it never resumes the target. request(str) returns decoded
bytes and enforces an exact read/control allowlist. request('c') returns b'' after
sending, like resume(); wait_stop(timeout) receives the later stop. interrupt()
sends the raw interrupt byte and waits for a stop. The parent must stop before
reads. read_memory() returns exact bytes; registers() returns XML-named x86
little-endian integers, requiring rax/rsp/rip/cs/cr3. Missing/unavailable registers
raise RegisterUnavailable, never an assumed offset/value. Hardware Z1/Z2 only.
read_register(name) reads just that XML-defined register using p, while stopped.
read_physical_memory() temporarily changes only QEMU's debugger address view,
restoring virtual mode in finally; physical-view query/set packets are the only
allowed QEMU extension. No packet writes guest memory or registers.

register_info maps names to immutable Register(number, bitsize); target_xml holds
raw XML by annex; capabilities holds qSupported fields; console holds decoded O
output; last_stop holds the latest raw decoded stop. JSONL logs append raw socket
chunks (hex), decoded packets, and state transitions with monotonic_ns. Logs are
host evidence only, not guest measurement evidence. close() only disconnects;
it neither resumes nor removes watchpoints (the runner owns their lifecycle).

Calls are serialized, including waits; use finite wait_stop followed by interrupt
from the same orchestration thread. A wait_stop timeout preserves buffered
bytes/run state and may be followed by another wait or interrupt. A timeout
during request/send, or any other protocol/transport failure, requires abandoning
the observation (apart from the physical-view restoration in finally). There are
no retransmission retries, no no-ack mode, and no software/monitor/write fallback.
"""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass
import json
import math
import os
import re
import socket
import threading
import time
import xml.etree.ElementTree as ET


class RspError(RuntimeError):
    """Transport, framing, target capability, or run-state failure."""


class RspTimeout(RspError):
    """A finite operation deadline expired; buffered input remains intact."""


class PolicyError(RspError):
    """A packet is outside the approved read-only observation policy."""


class RegisterUnavailable(RspError):
    """A required/XML-advertised register is missing or unavailable."""


@dataclass(frozen=True)
class Register:
    number: int
    bitsize: int


_HEX = r"[0-9a-fA-F]+"
_THREAD = rf"(?:-1|{_HEX}|p{_HEX}\.(?:-1|{_HEX}))"
_ALLOWED = re.compile(
    rf"(?:\?|qSupported(?::qXfer:features:read\+)?"
    rf"|qXfer:features:read:[A-Za-z0-9_.-]+:{_HEX},{_HEX}"
    rf"|qAttached(?::{_HEX})?|qC|H[gc]{_THREAD}|m{_HEX},{_HEX}"
    rf"|p{_HEX}|g|c|vCont\?|[Zz][12],{_HEX},{_HEX}"
    rf"|qqemu\.PhyMemMode|Qqemu\.PhyMemMode:[01])\Z"
)
_STOP = re.compile(rb"(?:S[0-9a-fA-F]{2}|T[0-9a-fA-F]{2}.*)\Z", re.DOTALL)
_EXIT = re.compile(rb"[WX][0-9a-fA-F]{2}(?:;.*)?\Z", re.DOTALL)
_MAX_PACKET = 4 * 1024 * 1024
_MAX_XML = 1024 * 1024
_XI = "{http://www.w3.org/2001/XInclude}include"


class RspClient:
    def __init__(self, socket_path, log_path, timeout=5.0):
        self.socket_path = os.fspath(socket_path)
        if not isinstance(self.socket_path, str) or not self.socket_path or "\x00" in self.socket_path:
            raise ValueError("socket_path must be a filesystem Unix socket path")
        self.log_path = os.fspath(log_path)
        self.timeout = self._duration(timeout)
        self.capabilities: dict[str, str | bool] = {}
        self.register_info: dict[str, Register] = {}
        self.target_xml: dict[str, bytes] = {}
        self.console: list[bytes] = []
        self.last_stop: bytes | None = None
        self._socket = None
        self._log = None
        self._buffer = bytearray()
        self._pending = deque()
        self._lock = threading.RLock()
        self._state = "disconnected"
        self._continue_ack = False
        self._packet_size = 4096
        self._hardware_breakpoints = set()

    @staticmethod
    def _duration(value):
        value = float(value)
        if not math.isfinite(value) or value <= 0:
            raise ValueError("timeout must be finite and positive")
        return value

    def _deadline(self, timeout=None):
        return time.monotonic() + (self.timeout if timeout is None else self._duration(timeout))

    def _record(self, event, **fields):
        if self._log is not None:
            self._log.write(json.dumps({"monotonic_ns": time.monotonic_ns(), "event": event, **fields}) + "\n")
            self._log.flush()

    def _require_socket(self):
        if self._socket is None:
            raise RspError("not connected")

    def _remaining(self, deadline):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise RspTimeout("RSP deadline expired")
        self._socket.settimeout(remaining)

    def _send(self, raw, deadline):
        self._require_socket()
        self._remaining(deadline)
        self._record("tx", raw_hex=raw.hex())
        try:
            self._socket.sendall(raw)
        except TimeoutError as exc:
            raise RspTimeout("RSP send timed out; delivery is uncertain") from exc
        except OSError as exc:
            raise RspError(f"RSP send failed: {exc}") from exc

    def _receive(self, deadline):
        self._remaining(deadline)
        try:
            raw = self._socket.recv(65536)
        except TimeoutError as exc:
            raise RspTimeout("RSP receive timed out") from exc
        except OSError as exc:
            raise RspError(f"RSP receive failed: {exc}") from exc
        if not raw:
            raise RspError("RSP peer disconnected")
        self._record("rx", raw_hex=raw.hex())
        self._buffer.extend(raw)

    @staticmethod
    def _decode(encoded):
        decoded = bytearray()
        index = 0
        while index < len(encoded):
            byte = encoded[index]
            index += 1
            if byte == ord('}'):
                if index == len(encoded):
                    raise RspError("truncated binary escape")
                decoded.append(encoded[index] ^ 0x20)
                index += 1
            elif byte == ord('*'):
                if not decoded or index == len(encoded):
                    raise RspError("invalid run-length encoding")
                count = encoded[index] - 29
                index += 1
                if not 3 <= count <= 97 or encoded[index - 1] in b'#$':
                    raise RspError("invalid run-length count")
                decoded.extend(bytes([decoded[-1]]) * count)
            else:
                decoded.append(byte)
            if len(decoded) > _MAX_PACKET:
                raise RspError("decoded packet exceeds observer limit")
        return bytes(decoded)

    def _event(self, deadline):
        while True:
            if not self._buffer:
                self._receive(deadline)
            first = self._buffer[0]
            if first in b'+-':
                del self._buffer[0]
                return bytes([first])
            if first != ord('$'):
                raise RspError(f"unexpected RSP framing byte {first:#x}")
            end = self._buffer.find(b'#', 1)
            if end == -1 or len(self._buffer) < end + 3:
                if len(self._buffer) > _MAX_PACKET:
                    raise RspError("wire packet exceeds observer limit")
                self._receive(deadline)
                continue
            if end > _MAX_PACKET:
                raise RspError("wire packet exceeds observer limit")
            encoded = bytes(self._buffer[1:end])
            checksum = bytes(self._buffer[end + 1:end + 3])
            del self._buffer[:end + 3]
            if not re.fullmatch(rb"[0-9a-fA-F]{2}", checksum) or sum(encoded) % 256 != int(checksum, 16):
                self._send(b'-', deadline)
                raise RspError("RSP checksum mismatch (no automatic retry)")
            self._send(b'+', deadline)
            result = self._decode(encoded)
            self._record("packet_rx", payload_hex=result.hex())
            return result

    def _console_packet(self, packet):
        if packet.startswith(b'O') and packet != b'OK':
            data = packet[1:]
            if len(data) % 2 or not re.fullmatch(rb"[0-9a-fA-F]*", data):
                raise RspError("malformed console O packet")
            decoded = bytes.fromhex(data.decode('ascii'))
            self.console.append(decoded)
            self._record("console", data_hex=decoded.hex())
            return True
        return False

    def _ack(self, deadline):
        while True:
            event = self._event(deadline)
            if event == b'+':
                return
            if event == b'-':
                raise RspError("target rejected packet checksum (no automatic retry)")
            if not self._console_packet(event):
                self._pending.append(event)

    def _reply(self, deadline):
        while True:
            event = self._pending.popleft() if self._pending else self._event(deadline)
            if event in (b'+', b'-'):
                raise RspError("unexpected acknowledgment")
            if self._console_packet(event):
                continue
            if not event:
                raise RspError("target reports unsupported packet")
            if re.fullmatch(rb"E[0-9a-fA-F]{2}(?:;.*)?", event):
                raise RspError(f"target error {event!r}")
            if _EXIT.fullmatch(event):
                self._state = "exited"
                self._record("state", state=self._state, reply_hex=event.hex())
                raise RspError(f"target exited: {event!r}")
            return event

    @staticmethod
    def _validate(command):
        if not isinstance(command, str) or not _ALLOWED.fullmatch(command):
            raise PolicyError(f"packet not allowed: {command!r}")
        if command.startswith(('Z', 'z', 'm', 'qXfer:')):
            if int(command.rsplit(',', 1)[1], 16) == 0:
                raise PolicyError("zero-length memory/feature/breakpoint request")

    def _packet(self, command, deadline):
        raw = command.encode('ascii')
        if len(raw) > self._packet_size:
            raise RspError("request exceeds target PacketSize")
        self._record("packet_tx", payload_hex=raw.hex())
        self._send(b'$' + raw + b'#' + f'{sum(raw) % 256:02x}'.encode('ascii'), deadline)

    def connect(self):
        with self._lock:
            if self._state != "disconnected":
                raise RspError("client cannot be connected twice; create a new client")
            # Append rather than truncating earlier failure evidence.
            self._log = open(self.log_path, 'a', encoding='utf-8')
            self._socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            try:
                self._socket.settimeout(self.timeout)
                self._socket.connect(self.socket_path)
                self._state = "initializing"
                self._record("state", state=self._state)
                supported = self.request('qSupported:qXfer:features:read+')
                for field in supported.decode('ascii').split(';'):
                    if '=' in field:
                        key, value = field.split('=', 1)
                        self.capabilities[key] = value
                    elif field.endswith(('+', '-')):
                        self.capabilities[field[:-1]] = field[-1] == '+'
                    elif field:
                        self.capabilities[field] = True
                if self.capabilities.get('qXfer:features:read') is not True:
                    raise RspError("target lacks qXfer:features:read+")
                if 'PacketSize' in self.capabilities:
                    value = self.capabilities['PacketSize']
                    if not isinstance(value, str) or not re.fullmatch(_HEX, value):
                        raise RspError("invalid advertised PacketSize")
                    self._packet_size = min(int(value, 16), _MAX_PACKET)
                    if self._packet_size < 64:
                        raise RspError("advertised PacketSize is too small")
                self.request('?')
                self._load_registers()
            except Exception:
                self.close()
                raise
        return self

    def request(self, command: str) -> bytes:
        self._validate(command)
        with self._lock:
            self._require_socket()
            if command == 'c':
                self.resume()
                return b''
            if self._state not in ('stopped', 'initializing'):
                raise RspError(f"request requires stopped target, state={self._state}")
            deadline = self._deadline()
            self._packet(command, deadline)
            self._ack(deadline)
            reply = self._reply(deadline)
            if command == '?':
                self._accept_stop(reply)
            elif _STOP.fullmatch(reply):
                self._accept_stop(reply)
                raise RspError("unexpected asynchronous stop during a read; response attribution lost")
            elif command.startswith(('Z', 'z', 'H', 'Qqemu.PhyMemMode:')) and reply != b'OK':
                raise RspError(f"target did not accept {command!r}: {reply!r}")
            return reply

    def read_physical_memory(self, addr, length) -> bytes:
        """Read physical backing while stopped; always restore virtual view.

        Initial physical mode is rejected rather than silently adopting an
        unknown debugger state. A restore failure is fatal and must terminate
        observation, including when it supersedes an earlier memory error.
        """
        self._address_length(addr, length)
        with self._lock:
            if self.request('qqemu.PhyMemMode') != b'0':
                raise RspError("expected initial virtual debugger memory view")
            try:
                self.request('Qqemu.PhyMemMode:1')
                return self.read_memory(addr, length)
            finally:
                self.request('Qqemu.PhyMemMode:0')

    def _accept_stop(self, reply):
        if not _STOP.fullmatch(reply):
            raise RspError(f"expected all-stop reply, got {reply!r}")
        self.last_stop = reply
        self._state = 'stopped'
        self._record("state", state=self._state, reply_hex=reply.hex())

    def resume(self) -> None:
        with self._lock:
            self._require_socket()
            if self._state != 'stopped':
                raise RspError(f"resume requires stopped target, state={self._state}")
            self._packet('c', self._deadline())
            self._continue_ack = True
            self._state = 'running'
            self._record("state", state=self._state)

    def step_over_breakpoint(self, addr) -> bytes:
        """Approved amendment: one native instruction, only current Z1 removed.

        Raw s/address/signal/vCont stepping remains outside request policy.
        Watchpoints and every other execute breakpoint remain installed.
        """
        with self._lock:
            self._require_socket()
            if self._state != 'stopped' or (addr, 1) not in self._hardware_breakpoints:
                raise PolicyError("step-over requires stopped enrolled current hardware breakpoint")
            if self.read_register('rip') != addr:
                raise PolicyError("step-over address is not current RIP")
            self.remove_breakpoint(addr)
            try:
                self._packet('s', self._deadline())
                self._continue_ack = True
                self._state = 'running'
                self._record('state', state=self._state, native_single_step=True)
                reply = self.wait_stop()
                if self.read_register('rip') == addr:
                    raise RspError("single native instruction did not advance current breakpoint")
                return reply
            finally:
                if self._state == 'running':
                    self.interrupt()
                if self._state == 'stopped':
                    self.add_breakpoint(addr)

    def wait_stop(self, timeout=None) -> bytes:
        with self._lock:
            self._require_socket()
            if self._state != 'running':
                raise RspError(f"wait_stop requires running target, state={self._state}")
            deadline = self._deadline(timeout)
            if self._continue_ack:
                self._ack(deadline)
                self._continue_ack = False
            reply = self._reply(deadline)
            self._accept_stop(reply)
            return reply

    def interrupt(self) -> bytes:
        with self._lock:
            self._require_socket()
            if self._state != 'running':
                raise RspError(f"interrupt requires running target, state={self._state}")
            self._send(b'\x03', self._deadline())
            return self.wait_stop()

    @staticmethod
    def _address_length(addr, length):
        if isinstance(addr, bool) or not isinstance(addr, int) or not 0 <= addr < 2**64:
            raise ValueError("address must be an unsigned 64-bit integer")
        if isinstance(length, bool) or not isinstance(length, int) or length <= 0 or addr + length > 2**64:
            raise ValueError("length must be positive and remain within 64-bit address space")

    def read_memory(self, addr, length) -> bytes:
        self._address_length(addr, length)
        with self._lock:
            result = bytearray()
            chunk_size = min(self._packet_size // 2, 65536)
            while len(result) < length:
                count = min(chunk_size, length - len(result))
                reply = self.request(f'm{addr + len(result):x},{count:x}')
                if len(reply) != count * 2 or not re.fullmatch(rb'[0-9a-fA-F]+', reply):
                    raise RspError("memory reply is not exact-length hex")
                result.extend(bytes.fromhex(reply.decode('ascii')))
            return bytes(result)

    def _read_xml(self, annex):
        data = bytearray()
        chunk = min(1024, (self._packet_size - 1) // 2)
        while True:
            reply = self.request(f'qXfer:features:read:{annex}:{len(data):x},{chunk:x}')
            if reply[:1] not in (b'm', b'l') or len(reply) - 1 > chunk:
                raise RspError("invalid feature XML chunk")
            data.extend(reply[1:])
            if len(data) > _MAX_XML:
                raise RspError("feature XML exceeds observer limit")
            if reply[:1] == b'l':
                self.target_xml[annex] = bytes(data)
                return bytes(data)
            if len(reply) == 1:
                raise RspError("nonterminal empty feature XML chunk")

    def _load_registers(self):
        next_number = 0
        numbers = set()
        active = set()
        architecture = []

        def visit(annex, depth=0):
            nonlocal next_number
            if depth > 16 or annex in active or len(self.target_xml) >= 128:
                raise RspError("recursive/excessive feature XML includes")
            if not re.fullmatch(r'[A-Za-z0-9_.-]+', annex):
                raise RspError("unsafe feature XML annex name")
            active.add(annex)
            raw = self._read_xml(annex)
            # ElementTree does not fetch the standard external gdb-target.dtd.
            # Reject entity declarations, avoiding internal entity expansion.
            if b'<!ENTITY' in raw.upper():
                raise RspError("XML entity declarations are not allowed")
            try:
                root = ET.fromstring(raw)
            except ET.ParseError as exc:
                # QEMU's GDB target.xml convention uses xi:include without a
                # namespace declaration; bind only that known prefix on target.
                # All entity/include/path/depth/register checks below still apply.
                if str(exc).startswith('unbound prefix:') and b'<target>' in raw and b'<xi:include ' in raw and b'xmlns:xi' not in raw:
                    try:
                        root = ET.fromstring(raw.replace(b'<target>', b'<target xmlns:xi="http://www.w3.org/2001/XInclude">', 1))
                    except ET.ParseError as fixed_exc:
                        raise RspError("malformed target XML") from fixed_exc
                else:
                    raise RspError("malformed target XML") from exc
            for node in root.iter():
                if node.tag == _XI:
                    if node.get('parse', 'xml') != 'xml' or node.get('xpointer') is not None:
                        raise RspError("unsupported XML include mode")
                    visit(node.get('href', ''), depth + 1)
                elif node.tag == 'architecture':
                    architecture.append((node.text or '').strip())
                elif node.tag == 'reg':
                    name = node.get('name')
                    try:
                        number = int(node.get('regnum', str(next_number)), 10)
                        bits = int(node.attrib['bitsize'], 10)
                    except (KeyError, ValueError) as exc:
                        raise RspError("invalid register description") from exc
                    if not name or name in self.register_info or number in numbers or not 0 <= number <= 65535 or bits <= 0 or bits % 8 or bits > 65536:
                        raise RspError("duplicate/invalid register description")
                    self.register_info[name] = Register(number, bits)
                    numbers.add(number)
                    next_number = number + 1
            active.remove(annex)

        visit('target.xml')
        if architecture != ['i386:x86-64']:
            raise RspError(f"expected explicit i386:x86-64 architecture, got {architecture!r}")
        missing = {'rax', 'rsp', 'rip', 'cs', 'cr3'} - self.register_info.keys()
        if missing:
            raise RegisterUnavailable(f"target XML lacks required registers: {', '.join(sorted(missing))}")

    @staticmethod
    def _register_value(name, reply, size):
        if len(reply) != size * 2:
            raise RegisterUnavailable(f"register {name} has wrong wire size")
        if b'x' in reply.lower():
            raise RegisterUnavailable(f"register {name} is unavailable")
        if not re.fullmatch(rb'[0-9a-fA-F]+', reply):
            raise RspError(f"register {name} is not hex")
        return int.from_bytes(bytes.fromhex(reply.decode('ascii')), 'little')

    def read_register(self, name: str) -> int:
        """Read one XML-named register via p; never assume a register number."""
        with self._lock:
            try:
                reg = self.register_info[name]
            except KeyError as exc:
                raise RegisterUnavailable(f"target XML lacks register {name!r}") from exc
            reply = self.request(f'p{reg.number:x}')
            return self._register_value(name, reply, reg.bitsize // 8)

    def registers(self) -> dict[str, int]:
        with self._lock:
            if not self.register_info:
                raise RegisterUnavailable("no XML register description loaded")
            ordered = sorted(self.register_info.items(), key=lambda pair: pair[1].number)
            result = {}
            # A gap makes the g layout ambiguous: use explicit XML p numbers.
            # Otherwise g describes a packed prefix; XML registers beyond that
            # prefix are read with p (not an unsupported-command fallback).
            contiguous = [reg.number for _, reg in ordered] == list(range(len(ordered)))
            packed = self.request('g') if contiguous else b''
            offset = 0
            for name, reg in ordered:
                size = reg.bitsize // 8
                if offset < len(packed):
                    value = packed[offset:offset + size * 2]
                    offset += size * 2
                else:
                    value = self.request(f'p{reg.number:x}')
                result[name] = self._register_value(name, value, size)
            if offset != len(packed):
                raise RspError("g reply extends beyond XML register layout")
            return result

    def _hardware(self, action, kind, addr, length):
        self._address_length(addr, length)
        self.request(f'{action}{kind},{addr:x},{length:x}')

    def add_breakpoint(self, addr, length=1):
        self._hardware('Z', 1, addr, length)
        self._hardware_breakpoints.add((addr, length))

    def remove_breakpoint(self, addr, length=1):
        self._hardware('z', 1, addr, length)
        self._hardware_breakpoints.discard((addr, length))

    def add_watchpoint(self, addr, length):
        self._hardware('Z', 2, addr, length)

    def remove_watchpoint(self, addr, length):
        self._hardware('z', 2, addr, length)

    def close(self):
        with self._lock:
            if self._socket is not None:
                self._socket.close()
                self._socket = None
            self._state = 'closed'
            self._record("state", state=self._state)
            if self._log is not None:
                self._log.close()
                self._log = None
