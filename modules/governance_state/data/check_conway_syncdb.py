# Standalone test for Conway governance correctness checking
# Use: check_conway_syncdb.py <dbsync-csv-output> <acropolis-conway-output>

import sys

def screen_strings(header):
    inside = False
    prev = " "
    out = ""
    for ptr in range(0,len(header)):
        if prev == '\"' and header[ptr] != '\"':
            inside = not inside

        if header[ptr] == ',' and inside:
            out += ';'
        else:
            out += header[ptr]
        prev = header[ptr]

    return out

def strip_some(s):
    return s.removeprefix('Some(').removesuffix(')')

if len(sys.argv) <= 2:
    print("\nUsage: %s <dbsync-csv-file> <acropolis-conway-output>\n" % sys.argv[0])
    exit(1)

dbsync = open(sys.argv[1],'rt')
acropolis = open(sys.argv[2],'rt')

dbsync_dict = {}

# 0 7 13 23
header = """id,tx_id,index,prev_gov_action_proposal,deposit,return_address,expiration,
    voting_anchor_id,type,description,param_proposal,ratified_epoch,enacted_epoch,
    dropped_epoch,expired_epoch,id,hash,block_id,block_index,out_sum,fee,deposit,size,
    invalid_before,invalid_hereafter,valid_contract,script_size,treasury_donation"""
header = screen_strings(header)
hl = header.split(',')

for s in dbsync:
    sh = screen_strings(s)
    sl = sh.strip().split(',')
    if len(sl) != len(hl):
        continue
    if sl == hl:
        continue

    hhash = sl[16].strip("\\x")

    idx = (hhash,sl[2])
    if idx not in dbsync_dict:
        dbsync_dict[idx] = []
    dbsync_dict[idx].append((sl[8],sl[11],sl[12],sl[13],sl[14]))

header_a = """id,start,tx_id,index,prev_gov_action_proposal,deposit,return_address,expiration,
              voting_anchor_id,type,description,param_proposal,ratified_epoch,enacted_epoch,
              dropped_epoch,expired_epoch"""
al = header_a.split(',')

found = 0
for s in acropolis:
    sh = screen_strings(s)
    sl = sh.strip().split(',')
    if len(sl) < 14:
        print("Rejecting:",s)
        continue
    if sl == al:
        continue

    idx = (sl[1],sl[2])
    if idx not in dbsync_dict:
        print(idx," not found\n")
    else:
        rec = dbsync_dict[idx]
        if len(rec) > 1:
            print(idx, "too many records: ",rec,"\n")

        found += 1
        (ty,re,en,de,ex) = rec[0]
        if ty == "InfoAction":
            ty = "Information"
        if ty != sl[8]: print (idx, "types do not match: `",ty,"` `",sl[8],"`\n")
        if re != strip_some(sl[11]): print (idx, "re do not match: `",re,"` `",sl[11],"`\n")
        if en != strip_some(sl[12]): print (idx, "en do not match: `",en,"` `",sl[12],"`\n")
        if ex != strip_some(sl[14]): print (idx, "ex do not match: `",ex,"` `",sl[14],"`\n")

print("Total found records: ",found,"\n")
