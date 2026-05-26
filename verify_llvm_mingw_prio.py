import ctypes
import os
import subprocess

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

# Run old DLL commits
def get_old_commits(date_order):
    cache_ptr = ctypes.c_void_p(old_dll.bt_new_commit_graph_cache(b"test"))
    token_ptr = ctypes.c_void_p(old_dll.bt_new_cancellation_token())
    storage = BtCommitStorage()
    git_dir = os.path.join(repo_dir, ".git").encode('utf-8')
    old_dll.bt_get_commits(git_dir, tips, len(shas), date_order, 2000, 0, 1, None, 0, ctypes.byref(cache_ptr), ctypes.byref(token_ptr), ctypes.byref(storage))
    oids = [str(storage.oids[i]) for i in range(storage.oids_len)]
    indexes = [storage.indexes[i] for i in range(storage.indexes_len)]
    res = []
    for i, idx in enumerate(indexes):
        commit_oid = oids[idx]
        next_idx = indexes[i+1] if i+1 < len(indexes) else len(oids)
        parents = oids[idx+1:next_idx]
        res.append((commit_oid, parents))
    return res

old_topo = get_old_commits(0)
old_date = get_old_commits(1)

commit_data = {}
for s in shas:
    stack = [s]
    while stack:
        curr = stack.pop()
        if curr in commit_data: continue
        out_cat = subprocess.check_output(f"git -C {repo_dir} cat-file -p {curr}", shell=True).decode()
        parents = []
        author_time = 0
        for line in out_cat.splitlines():
            if line.startswith("parent "):
                parents.append(line.split()[1])
            elif line.startswith("author "):
                parts = line.split(">")
                if len(parts) > 1:
                    rest = parts[1].strip()
                    author_time = int(rest.split()[0])
        commit_data[curr] = {"parents": parents, "time": author_time}
        for p in parents:
            stack.append(p)

def run_simulation(date_order, sign, oid_descending):
    record_indexes = {}
    records = []
    stack = list(shas)
    while stack:
        s = stack.pop()
        if s in record_indexes: continue
        record_indexes[s] = len(records)
        data = commit_data[s]
        records.append({"oid": s, "parents": data["parents"], "time": data["time"]})
        for p in reversed(data["parents"]):
            if p not in record_indexes:
                stack.append(p)
                
    remaining_children = [0] * len(records)
    for record in records:
        for p in record["parents"]:
            if p in record_indexes:
                remaining_children[record_indexes[p]] += 1
                
    ready = []
    for i in range(len(records)):
        if remaining_children[i] == 0:
            ready.append({"index": i, "priority": 0, "time": records[i]["time"], "oid": records[i]["oid"]})
            
    emitted = [False] * len(records)
    continuation_priority = [0] * len(records)
    emitted_count = 0
    flat = []
    
    while ready:
        def compare_items(a, b):
            if a["priority"] != b["priority"]:
                return 1 if a["priority"] > b["priority"] else -1
            if a["time"] != b["time"]:
                return 1 if a["time"] > b["time"] else -1
            if a["oid"] != b["oid"]:
                if oid_descending:
                    # Largest OID gets higher priority (returns 1 if a > b)
                    return 1 if a["oid"] > b["oid"] else -1
                else:
                    # Smaller OID gets higher priority (returns 1 if a < b)
                    return 1 if a["oid"] < b["oid"] else -1
            return 0
            
        from functools import cmp_to_key
        ready.sort(key=cmp_to_key(compare_items))
        item = ready.pop()
        
        record_index = item["index"]
        if emitted[record_index]: continue
        emitted[record_index] = True
        
        record = records[record_index]
        flat.append((record["oid"], record["parents"]))
        emitted_count += 1
        
        for parent_position, p in enumerate(record["parents"]):
            if p in record_indexes:
                parent_index = record_indexes[p]
                if remaining_children[parent_index] > 0:
                    remaining_children[parent_index] -= 1
                    if remaining_children[parent_index] == 0:
                        prio = emitted_count * 16 + (sign * parent_position)
                        continuation_priority[parent_index] = prio
                        ready.append({
                            "index": parent_index,
                            "priority": 0 if date_order else prio,
                            "time": records[parent_index]["time"],
                            "oid": records[parent_index]["oid"]
                        })
    return flat

for d_order in [0, 1]:
    for sign in [-1, 1]:
        for oid_desc in [False, True]:
            sim = run_simulation(d_order, sign, oid_desc)
            expected = old_date if d_order == 1 else old_topo
            match = sim == expected
            print(f"date_order={d_order}, sign={sign:+}, oid_descending={oid_desc} -> Match: {match}")
