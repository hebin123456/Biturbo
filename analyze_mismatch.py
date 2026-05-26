import ctypes
import os
import subprocess
import sys

old_dll = ctypes.CDLL(r"C:\develop\newfork\src\ForkPlus.Biturbo.Compare\bin\Debug\net10.0\biturbo.original.dll")

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
old_dll.bt_get_commits.argtypes = [
    ctypes.c_char_p, ctypes.POINTER(BtOid), ctypes.c_int64, ctypes.c_ubyte,
    ctypes.c_int64, ctypes.c_int64, ctypes.c_int64, ctypes.POINTER(BtOid), ctypes.c_int64,
    ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(BtCommitStorage)
]
old_dll.bt_get_commits.restype = ctypes.c_int32
old_dll.bt_new_commit_graph_cache.restype = ctypes.c_void_p
old_dll.bt_new_cancellation_token.restype = ctypes.c_void_p

repo_dir = r"C:\git\orm-cpp\llvm-mingw"
out = subprocess.check_output(f"git -C {repo_dir} show-ref", shell=True).decode()
shas = [line.split()[0] for line in out.splitlines() if line.strip()]
head = subprocess.check_output(f"git -C {repo_dir} rev-parse HEAD", shell=True).decode().strip()
shas.append(head)
shas = list(set(shas))
tips = (BtOid * len(shas))(*[parse_oid(sha) for sha in shas])

cache_ptr = ctypes.c_void_p(old_dll.bt_new_commit_graph_cache(b"test"))
token_ptr = ctypes.c_void_p(old_dll.bt_new_cancellation_token())
storage = BtCommitStorage()
git_dir = os.path.join(repo_dir, ".git").encode('utf-8')
old_dll.bt_get_commits(git_dir, tips, len(shas), 0, 2000, 0, 1, None, 0, ctypes.byref(cache_ptr), ctypes.byref(token_ptr), ctypes.byref(storage))

oids_old = [str(storage.oids[i]) for i in range(storage.oids_len)]
indexes_old = [storage.indexes[i] for i in range(storage.indexes_len)]
expected_commits = []
for i, idx in enumerate(indexes_old):
    commit_oid = oids_old[idx]
    expected_commits.append(commit_oid)

# Run fast parsing all commits with git log
commit_data = {}
log_out = subprocess.check_output(f"git -C {repo_dir} log --all --format=\"%H|%P|%at\"", shell=True).decode()
for line in log_out.splitlines():
    if not line.strip(): continue
    parts = line.split('|')
    s = parts[0]
    parents = parts[1].split() if parts[1] else []
    time = int(parts[2]) if parts[2] else 0
    commit_data[s] = {"parents": parents, "time": time}

# Run DFS to collect all reachable records from shas
record_indexes = {}
records = []
stack = list(shas)
while stack:
    s = stack.pop()
    if s in record_indexes: continue
    record_indexes[s] = len(records)
    data = commit_data.get(s, {"parents": [], "time": 0})
    records.append({"oid": s, "parents": data["parents"], "time": data["time"]})
    for p in reversed(data["parents"]):
        if p not in record_indexes:
            stack.append(p)

print("Expected commits length:", len(expected_commits))
print("DFS records length:", len(records))

expected_set = set(expected_commits)
records_set = set(r["oid"] for r in records)

print("In expected but not in DFS:", len(expected_set - records_set))
print("In DFS but not in expected:", len(records_set - expected_set))
