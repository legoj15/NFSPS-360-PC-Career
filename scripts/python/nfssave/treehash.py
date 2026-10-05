"""128-bit tree hash stored at tree[0:0x10] (reversed from nfs.exe 0x6D9CE0).

chained-MD5 (4 rounds over the raw digest) + RSA-style modpow with
exe-embedded exponent/modulus; input span = tree[0x10:tree_size] (the FULL
fixed device buffer). The verify path (0x5AABD0) is dead code in the PC
build — the loader never checks this hash — but we compute it anyway so
converted saves are indistinguishable from native ones.
"""

import hashlib

_E = bytes.fromhex(
    '41712698348b53b247ac4b0cfe32162265c1bdeb66590ed156707de646d307b4'
    '18e67f5a51c987be4b0bc80369920669e02bdcebfcdc40dba7169e1c7b22a62e')
_N = bytes.fromhex(
    '57a11e76c0fea0c76e43ac00cf073334444da3b91b462aa4bdfe3c389b383bb5'
    'a081c6d8d0b5ee6d1a2fcdf965a14743de47cc4f7eaf309e22cb6be37dcadaaf')


def tree_hash(tree: bytes) -> bytes:
    """16-byte value the game stores at tree[0:0x10]."""
    cur = hashlib.md5(tree[0x10:]).digest()
    parts = []
    for _ in range(4):
        parts.append(cur)
        cur = hashlib.md5(cur).digest()
    m = int.from_bytes(b"".join(parts), "little")
    r = pow(m, int.from_bytes(_E, "little"), int.from_bytes(_N, "little"))
    return r.to_bytes(0x82, "little")[0:0x10]
