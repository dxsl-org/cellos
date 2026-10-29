"""Host-only receive poll atomicity; original deadlines/policy/native guard unchanged."""
from pathlib import Path
import re
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import rsp as B


class AtomicReceiveMixin:
    def __init__(self, *args, **kwargs):
        self._pending_wire_frame = None
        self._wire_failure = None
        super().__init__(*args, **kwargs)

    def _require_socket(self):
        if self._wire_failure is not None:
            raise B.RspError(self._wire_failure)
        return super()._require_socket()

    def _fatal_wire(self, message):
        self._wire_failure = message
        self._record('wire_terminal', error=message)
        raise B.RspError(message)

    def _ack_frame(self, ack, deadline):
        # This preflight may expire benignly. No send has been attempted; the
        # complete validated frame and pending payload remain owned by us.
        self._require_socket()
        self._remaining(deadline)
        self._record('tx', raw_hex=ack.hex())
        try:
            result = self._socket.sendall(ack)
            if result is not None:
                self._fatal_wire('RSP ACK send returned partial/unknown delivery; terminal, no retry')
        except OSError as error:
            self._fatal_wire(f'RSP ACK send failed; delivery uncertain, terminal, no retry: {error}')

    def _event(self, deadline):
        self._require_socket()
        while True:
            if self._pending_wire_frame is None:
                if not self._buffer:
                    try:
                        self._receive(deadline)
                    except B.RspTimeout:
                        raise
                    except B.RspError as error:
                        self._fatal_wire(str(error))
                first = self._buffer[0]
                if first in b'+-':
                    del self._buffer[0]
                    return bytes([first])
                if first != ord('$'):
                    self._fatal_wire(f'unexpected RSP framing byte {first:#x}')
                end = self._buffer.find(b'#', 1)
                if end == -1 or len(self._buffer) < end + 3:
                    if len(self._buffer) > B._MAX_PACKET:
                        self._fatal_wire('wire packet exceeds observer limit')
                    try:
                        self._receive(deadline)
                    except B.RspTimeout:
                        raise
                    except B.RspError as error:
                        self._fatal_wire(str(error))
                    continue
                if end > B._MAX_PACKET:
                    self._fatal_wire('wire packet exceeds observer limit')
                encoded = bytes(self._buffer[1:end])
                checksum = bytes(self._buffer[end + 1:end + 3])
                if not re.fullmatch(rb'[0-9a-fA-F]{2}', checksum) or sum(encoded) % 256 != int(checksum, 16):
                    # Corrupt input is never a benign poll timeout, even if a NAK
                    # cannot be sent within this deadline. Never retry the frame.
                    try:
                        self._ack_frame(b'-', deadline)
                    except B.RspTimeout:
                        pass
                    self._fatal_wire('RSP checksum mismatch (no automatic retry)')
                try:
                    payload = self._decode(encoded)
                except B.RspError as error:
                    self._fatal_wire(str(error))
                self._pending_wire_frame = {'wire_length': end + 3, 'payload': payload,
                                            'ack_state': 'PENDING'}
                self._record('wire_frame_pending', payload_hex=payload.hex(), wire_length=end + 3)
            frame = self._pending_wire_frame
            self._ack_frame(b'+', deadline)
            frame['ack_state'] = 'ACKNOWLEDGED'
            # No deadline-dependent operation may intervene between ACK success,
            # consuming this frame and handing its payload to the original parser.
            del self._buffer[:frame['wire_length']]
            payload = frame['payload']
            self._pending_wire_frame = None
            self._record('packet_rx', payload_hex=payload.hex())
            return payload


class AtomicRspClient(AtomicReceiveMixin, B.RspClient):
    pass
