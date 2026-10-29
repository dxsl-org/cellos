"""S10-7.3: epoch-aware status selection (replaces the whole-screen single st=N rule).

The retained schedule runs `echo st=$?` after cancelled commands, so older
`st=0` / `st=130` rows can legitimately remain on screen. The row for THIS command is the
one printed after this command's own echo line. Fail closed on missing echo, no following
status row, or any extra status row after the echo (ambiguous/duplicate)."""
import re
CMD = 'echo st=$?'
_STATUS = re.compile(r'st=[0-9]+')


class StatusError(Exception):
    pass


def select_status(rows):
    rows = [r.strip() for r in rows]
    echoes = [i for i, r in enumerate(rows) if r.endswith(CMD)]
    if not echoes:
        raise StatusError(f'status echo row missing: {rows!r}')
    after = rows[echoes[-1] + 1:]
    vals = [r for r in after if _STATUS.fullmatch(r)]
    if len(vals) != 1:
        raise StatusError(f'expected exactly one status row after the last echo, got {vals!r}')
    return int(vals[0][3:])
