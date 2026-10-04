import sys, pickle
import exetools as E

def show(va, n=30, back=0):
    if back:
        va -= back
    for ins in E.disasm(va, n):
        print(f"  {ins.address:#x}: {ins.mnemonic:8s} {ins.op_str}")

if __name__ == '__main__':
    va = int(sys.argv[1], 16)
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 30
    show(va, n)
