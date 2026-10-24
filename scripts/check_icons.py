"""Validate the actual executable's embedded multi-size icon without opening a window."""
from pathlib import Path
import struct

root = Path(__file__).resolve().parents[1]
binary = (root / 'target/release/constellation.exe').read_bytes()
pe = struct.unpack_from('<I', binary, 0x3c)[0]
count = struct.unpack_from('<H', binary, pe + 6)[0]
optional = pe + 24
assert struct.unpack_from('<H', binary, optional)[0] == 0x20b, 'Expected PE32+'
table = optional + struct.unpack_from('<H', binary, pe + 20)[0]

def offset(rva):
    for i in range(count):
        size, address, raw_size, raw = struct.unpack_from('<IIII', binary, table + i * 40 + 8)
        if address <= rva < address + max(size, raw_size):
            return raw + rva - address
    raise AssertionError(f'Unmapped resource RVA {rva:x}')

rva, size = struct.unpack_from('<II', binary, optional + 112 + 2 * 8)
assert rva and size, 'Executable has no resources'
base = offset(rva)
resources = {}

def visit(relative, path=()):
    named, ids = struct.unpack_from('<HH', binary, base + relative + 12)
    for i in range(named + ids):
        name, entry = struct.unpack_from('<II', binary, base + relative + 16 + i * 8)
        if name & 0x80000000:
            continue
        key = path + (name,)
        if entry & 0x80000000:
            visit(entry & 0x7fffffff, key)
        else:
            data_rva, length = struct.unpack_from('<II', binary, base + entry)
            start = offset(data_rva)
            resources[key] = binary[start:start + length]

visit(0)
groups = [data for key, data in resources.items() if key[:2] == (14, 101)]
assert groups, 'Missing application icon group 101'
for group in groups:
    reserved, kind, entries = struct.unpack_from('<HHH', group)
    assert (reserved, kind, entries) == (0, 1, 9)
    sizes = set()
    for i in range(entries):
        width, height, _, _, planes, bits, length, icon_id = struct.unpack_from('<BBBBHHIH', group, 6 + i * 14)
        width, height = width or 256, height or 256
        assert width == height and planes == 1 and bits == 32
        payloads = [data for key, data in resources.items() if key[:2] == (3, icon_id)]
        assert payloads and all(len(data) == length for data in payloads)
        for data in payloads:
            assert data[:8] == b'\x89PNG\r\n\x1a\n'
            assert struct.unpack_from('>II', data, 16) == (width, height)
            assert data[24:26] == bytes([8, 6]), 'Expected 8-bit RGBA PNG'
        sizes.add(width)
    assert sizes == {16, 20, 24, 32, 40, 48, 64, 128, 256}
print('Embedded application icon verified: nine RGBA sizes, 16–256 px; no desktop interaction.')
