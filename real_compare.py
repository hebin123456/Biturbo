import ctypes
import os
import subprocess

old_dll = ctypes.CDLL(r"C:\develop\newfork\src\ForkPlus.Biturbo.Compare\bin\Debug\net10.0\biturbo.original.dll")
new_dll = ctypes.CDLL(r"C:\develop\biturbo\target\release\biturbo.dll")

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

class BtCommitStorage(ctypes.Structure):
    _fields_ = [
        ("oids", ctypes.POINTER(BtOid)),
        ("oids_len", ctypes.c_int64),
        ("oids_cap", ctypes.c_int64),
        ("indexes", ctypes.POINTER(ctypes.c_uint32)),
        ("indexes_len", ctypes.c_int64),
        ("indexes_cap", ctypes.c_int64),
        ("has_more", ctypes.c_ubyte)
    ]

# Set argtypes
for dll_lib in [old_dll, new_dll]:
    dll_lib.bt_get_commits.argtypes = [
        ctypes.c_char_p,                  # git_dir_path
        ctypes.POINTER(BtOid),            # tips_ptr
        ctypes.c_int64,                   # tips_len
        ctypes.c_ubyte,                   # date_order
        ctypes.c_int64,                   # page_size
        ctypes.c_int64,                   # skip_pages
        ctypes.c_int64,                   # min_pages
        ctypes.POINTER(BtOid),            # required_oids_ptr
        ctypes.c_int64,                   # required_oids_len
        ctypes.POINTER(ctypes.c_void_p),  # commit_graph_cache_ptr
        ctypes.POINTER(ctypes.c_void_p),  # cancellation_token_ptr
        ctypes.POINTER(BtCommitStorage)   # out_result
    ]
    dll_lib.bt_get_commits.restype = ctypes.c_int32
    dll_lib.bt_new_commit_graph_cache.restype = ctypes.c_void_p
    dll_lib.bt_new_cancellation_token.restype = ctypes.c_void_p

repo_dir = r"C:\git\orm-cpp\llvm-mingw"
git_dir = r"C:\git\orm-cpp\.git\modules\llvm-mingw".encode('utf-8')

# Get references
shas = []
out = subprocess.check_output(f"git -C {repo_dir} show-ref", shell=True).decode()
for line in out.splitlines():
    if line.strip(): shas.append(line.split()[0])
try:
    head = subprocess.check_output(f"git -C {repo_dir} rev-parse HEAD", shell=True).decode().strip()
    shas.append(head)
except Exception:
    pass

shas = list(set(shas))
tips_arr = (BtOid * len(shas))(*[parse_oid(sha) for sha in shas])

def get_commits_from_dll(dll_lib, date_order):
    cache_ptr = ctypes.c_void_p(dll_lib.bt_new_commit_graph_cache(b"test"))
    token_ptr = ctypes.c_void_p(dll_lib.bt_new_cancellation_token())
    storage = BtCommitStorage()
    
    rc = dll_lib.bt_get_commits(
        git_dir,
        tips_arr, len(shas),
        date_order,
        2000, # page_size
        0, # skip_pages
        1, # min_pages
        None, 0,
        ctypes.byref(cache_ptr),
        ctypes.byref(token_ptr),
        ctypes.byref(storage)
    )
    oids = [str(storage.oids[i]) for i in range(storage.oids_len)]
    indexes = [storage.indexes[i] for i in range(storage.indexes_len)]
    res = []
    for i, idx in enumerate(indexes):
        commit_oid = oids[idx]
        next_idx = indexes[i+1] if i+1 < len(indexes) else len(oids)
        parents = oids[idx+1:next_idx]
        res.append(f"{commit_oid} -> {parents}")
    return res

for d_order in [0, 1]:
    mode_name = "DATE_ORDER" if d_order == 1 else "TOPO_ORDER"
    print(f"=== Comparing {mode_name} with REAL ORIGINAL DLL ===")
    old_commits = get_commits_from_dll(old_dll, d_order)
    new_commits = get_commits_from_dll(new_dll, d_order)
    
    if old_commits == new_commits:
        print(f"  Match on {mode_name}!")
    else:
        print(f"  Mismatch on {mode_name}!")
        print(f"  Old len: {len(old_commits)}, New len: {len(new_commits)}")
        mismatches_printed = 0
        for i in range(max(len(old_commits), len(new_commits))):
            line1 = old_commits[i] if i < len(old_commits) else ""
            line2 = new_commits[i] if i < len(new_commits) else ""
            if line1 != line2:
                print(f"  Diff at index {i}:")
                print(f"    Old: {line1}")
                print(f"    New: {line2}")
                mismatches_printed += 1
                if mismatches_printed >= 10:
                    break
