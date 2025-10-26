import base64
import json
import re

key_type = {
    'ConstitutionalCommitteeKey': 3,
    'ConstitutionalCommitteeScript': 4,
    'DRepKey': 0, 
    'DRepScript': 1, 
    'StakePoolKey': 2
}

def array_as_base64(keyhash):
    bs = base64.b64encode(bytes(keyhash))
    return bs.decode('ascii')

all_voters_hash = {}

def split_votes(vstr):
    # DRepKey([...]): ([...], VotingProcedure { vote: Yes, anchor: (...), vote_index: 0 }) ,
    vlist = re.split(r'vote_index: \d+ \}\), +', vstr)
    votes = { 'Yes': [], 'No': [], 'Abstain': [] }
    for velem in vlist:
        print(velem)
        g = re.match(r'([a-zA-Z]+)\(\[([,0-9 ]+)\]\): \(.*, VotingProcedure \{ vote: (Yes|No|Abstain).*', velem)
        if g:
            key = array_as_base64([int(x) for x in g.group(2).split(', ')])
            all_voters_hash[key] = 1
            votes[g.group(3)] += [(key_type[g.group(1)], key)]
    return votes

votes_src = open('execution_log.txt', 'rt')
votes_hash = {}
param_hash = {}
epoch_data = []

for v in votes_src:
    g = re.match(r'.*Epoch start (\d+), (gov_action1[^:]+):  \([0-9]+, .*\)  => {(.+)}$',v)
    if g:
        if int(g.group(1))-1 not in votes_hash:
            votes_hash[int(g.group(1))-1] = []
        votes_hash[int(g.group(1))-1] += [(g.group(2), split_votes(g.group(3)))]

    g = re.match(r'.*acropolis_module_parameters_state: NPPX: \[(\d+),(.*)\]$',v)
    if g:
        param_hash[int(g.group(1))] = g.group(2)

    # acropolis_module_governance_state::state: Conway voting, epoch 508 (Conway): 
    # spos reg. 23814460205033971, dreps 0 (no-confidence 0, abstain 0), committee 7, total 0 actions, 0 accepted
    g = re.match(r'.*acropolis_module_governance_state::state: Conway voting, epoch (\d+) .*' +
                 r'spos reg. (\d+), dreps (\d+) \(no-confidence (\d+), abstain (\d+)\), committee (\d+),',v)
    if g:
        epoch_data += [(int(g.group(1)), int(g.group(2)), int(g.group(3)), int(g.group(4)), int(g.group(5)))]
        if g.group(6) != '7':
            print("Wrong committee size in epoch %s: %s" % (g.group(1), g.group(6)))
            sys.exit(1)

#print(votes_hash)

out_d = open('drep_state.json', 'wt')
out_p = open('pool_state.json', 'wt')
out_v = open('voting_state.json', 'wt')
out_c = open('param_state.json', 'wt')
out_e = open('epoch_pool_stats.json', 'wt')

def convert_drep(ee):
    src = open('drep/drep-%d.json' % ee, 'rt')
    data = json.load(src)

    dreps_res = []

    epoch = int(data['Cardano'][1]['DRepStakeDistribution']['epoch'])

    dreps_raw = data['Cardano'][1]['DRepStakeDistribution']['drdd']['dreps']
    for drep in dreps_raw:
        if 'AddrKeyHash' in drep[0]:
            keytype = 0
            keyhash = drep[0]['AddrKeyHash']

        if 'ScriptHash' in drep[0]:
            keytype = 1
            keyhash = drep[0]['ScriptHash']

        lovelace = drep[1]
        key_b64 = array_as_base64(keyhash)

        if key_b64 in all_voters_hash:
            dreps_res += ["[%d,\"%s\",%s]" % (keytype, key_b64, lovelace)]

    return (epoch+1, ','.join(dreps_res))

def convert_pool(ee):
    src = open('pool/spo-%d.json' % ee, 'rt')
    data = json.load(src)

    pool_res = []

    epoch = int(data['Cardano'][1]['SPOStakeDistribution']['epoch'])

    pool_raw = data['Cardano'][1]['SPOStakeDistribution']['spos']
    for pool in pool_raw:
        keyhash = pool[0]

        active = pool[1]['active']
        live = pool[1]['live']
        key_b64 = array_as_base64(keyhash)

        if key_b64 in all_voters_hash:
            pool_res += ["[\"%s\",%s,%s]" % (key_b64, active, live)]

    return (epoch+1, ','.join(pool_res))

def convert_votes(ee):
    res = []
    if ee in votes_hash:
        for (gov,vv) in votes_hash[ee]:
            vv_yes = ",".join(["[%s,\"%s\"]" % (t,s) for (t,s) in vv['Yes']])
            vv_no = ",".join(["[%s,\"%s\"]" % (t,s) for (t,s) in vv['No']])
            vv_abstain = ",".join(["[%s,\"%s\"]" % (t,s) for (t,s) in vv['Abstain']])
            res += [(ee,gov,("[%s],[%s],[%s]" % (vv_yes, vv_no, vv_abstain)))]
    return res

out_d.write('[')
out_p.write('[')
out_v.write('[')
first_d = False
first_p = False
first_v = False
for ee in range(507,575):
    if first_d: out_d.write(',')
    first_d = True
    if first_p: out_p.write(',')
    first_p = True
    out_d.write('[%d,[%s]]\n' % convert_drep(ee))
    out_p.write('[%d,[%s]]\n' % convert_pool(ee))
    for vv in convert_votes(ee):
        if first_v: out_v.write(',')
        first_v = True
        out_v.write('[%d,\"%s\",[%s]]\n' % vv)

out_d.write(']\n')
out_p.write(']\n')
out_v.write(']\n')

out_c.write('[')
first_c = False
for x in param_hash.keys():
    out_c.write('%s[%s,%s]\n' % ((',' if first_c else ''),x,param_hash[x]))
    first_c = True
out_c.write(']\n')

out_e.write('[')
first_e = False
for ed in sorted(epoch_data):
    if first_e: out_e.write(',')
    first_e = True
    out_e.write('[%d,%d,%d,%d,%d]\n' % ed)
out_e.write(']\n')
