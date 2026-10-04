"""
tree_hash.py - reimplementation of the 128-bit tree hash stored at tree[0:0x10]
in NFS ProStreet PC saves (fn VA 0x6D9CE0 in nfs.exe).

ALGORITHM (verified against both sample saves byte-for-byte):

  1. h1 = MD5-BE(tree[0x10 : tree_size])
         Round 1 = STANDARD MD5 of the span (verified byte-exact against the
         emulated transform at VA 0x6CFE60 - it is plain MD5, not a variant).
  2. hex = h1.hex() (lowercase ASCII)
  3. Chain: the 16 RAW digest bytes are fed back into the same hash object;
     "Update after Finalize" goes through SecuROM thunk [0x19E787C] which
     fully RESETS the object (IV, finalized flag AND the byte-length counter,
     replicated by the reset stub at 0x6D3F36+0x10 area), so each round is a
     fresh MD5 of the previous 16-byte digest. Four rounds total.
  4. local = concat of the four 16-byte digests = 64 raw bytes (the finalized
     object also keeps the raw digest at ctx+0x59 and its hex at ctx+0x69)
  5. M = little-endian integer of those 64 bytes (bigint ctor VA 0x6D4A30 maps
     byte i -> limb[i>>1] |= byte << ((i&1)*8), limbs are little-endian u16s)
  6. R = M ^ E mod N   (square-and-multiply modpow, VA 0x6D6BA0, bit loop over
        the exponent, acc starts at 1; multiply VA 0x6D4AA0, modulo VA 0x6D6350)
        E = 0x40-byte exponent table at nfs.exe VA 0x98CF88
        N = 0x40-byte modulus  table at nfs.exe VA 0x98CF48
  7. stored tree[0:0x10] = R.to_bytes(0x82,'little')[0:0x10]
     (the 0x40 low bytes of the bigint result are copied back into the local
      buffer by VA 0x6D8DD0; the hash stores the FIRST 16 bytes of that copy)

Hashed span on write (serializer VA 0x5AACC0 / call site 0x5AADD0-0x5AADDA)
and on the verify path (VA 0x5AABD0): tree[0x10 : tree_size] - the FULL device
buffer from offset 0x10 to its fixed size (0x5000 alias / 0xB6800 career),
including stale bytes and allocator fill. NOTE: the verify function 0x5AABD0
has NO references anywhere in the exe (no calls, no function pointers) - it is
dead code in the PC build; the load path never checks this hash. Only the
MC02 CRCs (header/extra/tree) are validated (VA 0x89F812 state machine).
"""
import struct, math, sys

import struct, sys, hashlib

def md5_be(msg: bytes) -> bytes:
    """The transform (VA 0x6CFE60) computes STANDARD MD5 (verified against the
    emulated code; the static big-endian-looking word walk is misleading)."""
    return hashlib.md5(msg).digest()


# ---------------------------------------------------------------- final mix
E_TABLE = bytes.fromhex(      # exponent, nfs.exe VA 0x98CF88
    '41712698348b53b247ac4b0cfe32162265c1bdeb66590ed156707de646d307b4'
    '18e67f5a51c987be4b0bc80369920669e02bdcebfcdc40dba7169e1c7b22a62e')
N_TABLE = bytes.fromhex(      # modulus, nfs.exe VA 0x98CF48
    '57a11e76c0fea0c76e43ac00cf073334444da3b91b462aa4bdfe3c389b383bb5'
    'a081c6d8d0b5ee6d1a2fcdf965a14743de47cc4f7eaf309e22cb6be37dcadaaf')

def tree_hash(tree: bytes) -> bytes:
    """16-byte value the game stores at tree[0:0x10]. `tree` = whole buffer."""
    cur = md5_be(tree[0x10:])            # round 1: full buffer past the hash field
    parts = []
    for _ in range(4):
        parts.append(cur)                # 16 RAW digest bytes are fed back
        cur = md5_be(cur)                # each round hashes the previous digest
    local = b''.join(parts)              # 64 raw bytes
    M = int.from_bytes(local, 'little')
    R = pow(M, int.from_bytes(E_TABLE, 'little'), int.from_bytes(N_TABLE, 'little'))
    return R.to_bytes(0x82, 'little')[0:0x10]

# ---------------------------------------------------------------- self test
def _load_tree(path):
    d = open(path, 'rb').read()
    extra = struct.unpack_from('<I', d, 8)[0]
    return d[0x1C + extra:]

if __name__ == '__main__':
    base = ("E:/legoj/Documents/Need for Speed ProStreet/"
            "100% Gamesave (OPTIONAL) - Place This in SAVE folder below/NFS Prostreet/")
    files = [("ALIAS_PEIROKUNMANWSP/ALIAS_PEIROKUNMANWSP", "alias"),
             ("CAREER_01/CAREER_01", "career")]
    ok = True
    for path, tag in files:
        tree = _load_tree(base + path)
        stored = tree[0:16]
        computed = tree_hash(tree)
        match = stored == computed
        ok &= match
        print(f"{tag:7s} stored   = {stored.hex()}")
        print(f"{tag:7s} computed = {computed.hex()}   {'MATCH' if match else 'MISMATCH'}")
    sys.exit(0 if ok else 1)
