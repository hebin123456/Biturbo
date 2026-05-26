import ctypes
import os
import subprocess

old_dll = ctypes.CDLL(r"C:\develop\newfork\src\ForkPlus.Biturbo.Compare\bin\Debug\net10.0\biturbo.original.dll")
new_dll = ctypes.CDLL(r"C:\develop\newfork\src\ForkPlus.Biturbo\bin\Debug\biturbo.dll")

class BtOid(ctypes.Structure):
    _fields_ = [
        ("s0", ctypes.c_uint32),
        ("s1", ctypes.c_uint32),
        ("s2", ctypes.c_uint32),
        ("s3", ctypes.c_uint32),
        ("s4", ctypes.c_uint32)
    ]
    def __str__(self):
        return f"{self.s0:08x}{self.s1:08x}{self.s2:08x}{self.s3:08x}{self.s4:08x}"

def parse_oid(sha):
    b = bytes.fromhex(sha)
    return BtOid(
        int.from_bytes(b[0:4], 'big'),
        int.from_bytes(b[4:8], 'big'),
        int.from_bytes(b[8:12], 'big'),
        int.from_bytes(b[12:16], 'big'),
        int.from_bytes(b[16:20], 'big')
    )

class BtBuf(ctypes.Structure):
    _fields_ = [
        ("ptr", ctypes.c_void_p),
        ("len", ctypes.c_uint64),
        ("cap", ctypes.c_uint64)
    ]

class BtReferences(ctypes.Structure):
    _fields_ = [
        ("a", BtBuf),
        ("b", BtBuf),
        ("c", BtBuf),
        ("d", BtBuf),
        ("e", BtBuf),
        ("hash", ctypes.c_uint64)
    ]

# Set argtypes for bt_get_references
for d in [old_dll, new_dll]:
    d.bt_get_references.argtypes = [ctypes.c_char_p, ctypes.c_ubyte, ctypes.POINTER(BtReferences)]
    d.bt_get_references.restype = ctypes.c_int32
    d.bt_release_references.argtypes = [ctypes.POINTER(BtReferences)]

repo_dir = r"C:\git\orm-cpp\llvm-mingw"
# Resolving git_dir path like C# does
git_dir = r"C:\git\orm-cpp\.git\modules\llvm-mingw".encode('utf-8')

# Compare bt_get_references
refs_old = BtReferences()
refs_new = BtReferences()

rc_old = old_dll.bt_get_references(git_dir, 1, ctypes.byref(refs_old)) # include_tags=1 (means skip tags, so branches only)
rc_new = new_dll.bt_get_references(git_dir, 1, ctypes.byref(refs_new))

print("Refs RC match (skip tags):", rc_old == rc_new)
print("Refs hash match (skip tags):", refs_old.hash == refs_new.hash)

def get_ref_data(refs):
    if refs.b.len == 0:
        return []
    names_offsets = ctypes.cast(refs.b.ptr, ctypes.POINTER(ctypes.c_int64))
    names_offsets_len = refs.b.len
    names_bytes = ctypes.string_at(refs.a.ptr, refs.a.len)
    
    offsets = [names_offsets[i] for i in range(names_offsets_len)]
    names = []
    start = 0
    for end in offsets:
        names.append(names_bytes[start:end].decode('utf-8'))
        start = end
        
    oids_ptr = ctypes.cast(refs.c.ptr, ctypes.POINTER(BtOid))
    oids = [str(oids_ptr[i]) for i in range(refs.c.len)]
    
    return list(zip(names, oids))

old_list = get_ref_data(refs_old)
new_list = get_ref_data(refs_new)

print("Old refs length:", len(old_list))
print("New refs length:", len(new_list))

if old_list == new_list:
    print("References match 100%!")
else:
    print("References MISMATCH!")
    for i, (item1, item2) in enumerate(zip(old_list, new_list)):
        if item1 != item2:
            print(f"Diff at index {i}:")
            print("  Old:", item1)
            print("  New:", item2)
            break

old_dll.bt_release_references(ctypes.byref(refs_old))
new_dll.bt_release_references(ctypes.byref(refs_new))
