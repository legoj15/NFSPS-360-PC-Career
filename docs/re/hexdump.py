import sys, struct

def hexdump(b, start=0, length=None, width=16):
    if length is None: length = len(b)
    for off in range(start, min(start+length, len(b)), width):
        chunk = b[off:off+width]
        hx = ' '.join(f'{c:02x}' for c in chunk)
        asc = ''.join(chr(c) if 32 <= c < 127 else '.' for c in chunk)
        print(f"  {off:06x}: {hx:<{width*3}} {asc}")

if __name__ == '__main__':
    p = sys.argv[1]
    d = open(p,'rb').read()
    s = int(sys.argv[2],0) if len(sys.argv)>2 else 0
    l = int(sys.argv[3],0) if len(sys.argv)>3 else 0x200
    hexdump(d, s, l)
