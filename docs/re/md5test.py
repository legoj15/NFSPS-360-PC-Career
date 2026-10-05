import struct, math

MASK = 0xFFFFFFFF
def rol(x, n): return ((x << n) | (x >> (32 - n))) & MASK

K = [int(abs(math.sin(i + 1)) * (1 << 32)) & MASK for i in range(64)]
S = [7,12,17,22]*4 + [5,9,14,20]*4 + [4,11,16,23]*4 + [6,10,15,21]*4

def md5variant(msg, be_x=False, pad80=False, hex_out=False):
    # NOTE: length append must match original bit count of msg BEFORE padding
    orig_len_bits = (len(msg) * 8) & 0xFFFFFFFFFFFFFFFF  # 64-bit LE at end
    # big-endian X load => we byte-swap each word when loading
    if pad80:
        pad = b'\x80' * ((-(len(msg) - 56)) % 64 if len(msg) % 64 < 56 else ((-(len(msg)+1-56))%64) + 1)
        # simpler: append 0x80s until len%64 == 56
        buf = bytearray(msg)
        while len(buf) % 64 != 56:
            buf.append(0x80)
        buf += struct.pack('<Q', orig_len_bits)
        msg = bytes(buf)
    else:
        buf = bytearray(msg)
        buf.append(0x80)
        while len(buf) % 64 != 56:
            buf.append(0)
        buf += struct.pack('<Q', orig_len_bits)
        msg = bytes(buf)

    a0,b0,c0,d0 = 0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476
    for off in range(0, len(msg), 64):
        X = list(struct.unpack_from('<16I', msg, off))
        if be_x:
            X = [struct.unpack('<I', struct.pack('>I', x))[0] for x in X]
        A,B,C,D = a0,b0,c0,d0
        for i in range(64):
            if i < 16: F = (B & C) | (~B & D); g = i
            elif i < 32: F = (D & B) | (~D & C); g = (5*i+1) % 16
            elif i < 48: F = B ^ C ^ D;         g = (3*i+5) % 16
            else:       F = C ^ (B | ~D);       g = (7*i) % 16
            F = (F + A + K[i] + X[g]) & MASK
            A = D; D = C; C = B
            B = (B + rol(F, S[i])) & MASK
        a0 = (a0 + A) & MASK; b0 = (b0 + B) & MASK
        c0 = (c0 + C) & MASK; d0 = (d0 + D) & MASK
    digest = struct.pack('<4I', a0, b0, c0, d0)
    return digest.hex().encode() if hex_out else digest

if __name__ == '__main__':
    import hashlib
    assert md5variant(b'') == hashlib.md5(b'').digest(), "baseline md5 broken"
    print("baseline MD5 OK")

    base = "E:/legoj/Documents/Need for Speed ProStreet/100% Gamesave (OPTIONAL) - Place This in SAVE folder below/NFS Prostreet/"
    for tag, path in [("alias","ALIAS_PEIROKUNMANWSP/ALIAS_PEIROKUNMANWSP"),
                      ("career","CAREER_01/CAREER_01")]:
        d = open(base+path,'rb').read()
        extra = struct.unpack_from('<I', d, 8)[0]
        tree = d[0x1c+extra:]
        stored = tree[0:16]
        span = tree[0x10:]
        print(f"{tag}: stored={stored.hex()}")
        for name, kw in [("plain MD5", {}), ("BE-X", dict(be_x=True)),
                         ("pad80", dict(pad80=True)), ("BE-X+pad80", dict(be_x=True, pad80=True))]:
            got = md5variant(span, **kw)
            mark = "MATCH!" if got == stored else ""
            print(f"   {name:12s} {got.hex()} {mark}")
