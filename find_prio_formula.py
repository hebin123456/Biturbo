import ctypes
import os
import subprocess

real_old_dll = ctypes.CDLL(r"C:\develop\biturbo\biturbo.dll")

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
real_old_dll.bt_get_commits.argtypes = [
    ctypes.c_char_p, ctypes.POINTER(BtOid), ctypes.c_int64, ctypes.c_ubyte,
    ctypes.c_int64, ctypes.c_int64, ctypes.c_int64, ctypes.POINTER(BtOid), ctypes.c_int64,
    ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(BtCommitStorage)
]
real_old_dll.bt_get_commits.restype = ctypes.c_int32
real_old_dll.bt_new_commit_graph_cache.restype = ctypes.c_void_p
real_old_dll.bt_new_cancellation_token.restype = ctypes.c_void_p

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

def get_real_commits(date_order):
    cache_ptr = ctypes.c_void_p(real_old_dll.bt_new_commit_graph_cache(b"test"))
    token_ptr = ctypes.c_void_p(real_old_dll.bt_new_cancellation_token())
    storage = BtCommitStorage()
    real_old_dll.bt_get_commits(git_dir, tips_arr, len(shas), date_order, 2000, 0, 1, None, 0, ctypes.byref(cache_ptr), ctypes.byref(token_ptr), ctypes.byref(storage))
    oids = [str(storage.oids[i]) for i in range(storage.oids_len)]
    indexes = [storage.indexes[i] for i in range(storage.indexes_len)]
    res = []
    for i, idx in enumerate(indexes):
        commit_oid = oids[idx]
        next_idx = indexes[i+1] if i+1 < len(indexes) else len(oids)
        parents = oids[idx+1:next_idx]
        res.append((commit_oid, parents))
    return res

real_topo = get_real_commits(0)
real_date = get_real_commits(1)

print("Real TOPO length:", len(real_topo))
print("Real DATE length:", len(real_date))

# Load commit data
commit_data = {}
log_out = subprocess.check_output(f"git -C {repo_dir} log --all --format=\"%H|%P|%at\"", shell=True).decode()
for line in log_out.splitlines():
    if not line.strip(): continue
    parts = line.split('|')
    s = parts[0]
    parents = parts[1].split() if parts[1] else []
    time = int(parts[2]) if parts[2] else 0
    commit_data[s] = {"parents": parents, "time": time}

def run_simulation(date_order, formula, oid_descending, use_prio_for_date):
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
                    return 1 if a["oid"] > b["oid"] else -1
                else:
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
                        prio = formula(len(records), emitted_count, parent_position)
                        continuation_priority[parent_index] = prio
                        
                        is_date_order = date_order == 1
                        actual_prio = prio
                        if is_date_order and not use_prio_for_date:
                            actual_prio = 0
                            
                        ready.append({
                            "index": parent_index,
                            "priority": actual_prio,
                            "time": records[parent_index]["time"],
                            "oid": records[parent_index]["oid"]
                        })
    return flat

# Define formulas to test
formulas = [
    ("C++ standard", lambda R, emitted, pos: (R - emitted) * 16 - pos),
    ("C++ pos addition", lambda R, emitted, pos: (R - emitted) * 16 + pos),
    ("Rust standard", lambda R, emitted, pos: emitted * 16 + pos),
    ("Rust pos subtraction", lambda R, emitted, pos: emitted * 16 - pos),
]

print("\n=== SEARCHING FOR MATCHES ===", flush=True)
for d_order in [0, 1]:
    expected = real_date if d_order == 1 else real_topo
    mode_name = "DATE_ORDER" if d_order == 1 else "TOPO_ORDER"
    found_any = False
    
    for name, f in formulas:
        for oid_desc in [False, True]:
            for use_prio_date in [False, True]:
                if d_order == 0 and use_prio_date:
                    continue # only applicable for date_order
                sim = run_simulation(d_order, f, oid_desc, use_prio_date)
                sim_trunc = sim[:len(expected)]
                if sim_trunc == expected:
                    print(f"MATCH FOUND for {mode_name}!")
                    print(f"  Formula: {name}")
                    print(f"  OID Descending: {oid_desc}")
                    if d_order == 1:
                        print(f"  Use Priority for Date Order: {use_prio_date}")
                    found_any = True
    if not found_any:
        print(f"NO MATCH found for {mode_name}!")
