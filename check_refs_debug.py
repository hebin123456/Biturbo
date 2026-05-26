import ctypes
import os

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

repo_dir = r"C:\git\orm-cpp\llvm-mingw"
git_dir = os.path.join(repo_dir, ".git").encode('utf-8')

# Compare bt_get_references
refs_old = BtReferences()
refs_new = BtReferences()

rc_old = old_dll.bt_get_references(git_dir, 0, ctypes.byref(refs_old))
rc_new = new_dll.bt_get_references(git_dir, 0, ctypes.byref(refs_new))

print("rc_old:", rc_old)
print("rc_new:", rc_new)
print("Refs hash match:", refs_old.hash == refs_new.hash)
print("Old hash:", refs_old.hash)
print("New hash:", refs_new.hash)
print("Old names_data_len:", refs_old.a.len)
print("New names_data_len:", refs_new.a.len)
print("Old oids_len:", refs_old.c.len)
print("New oids_len:", refs_new.c.len)
