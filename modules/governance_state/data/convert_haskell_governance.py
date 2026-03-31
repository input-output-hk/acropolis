# The file contains the data lines, which are in the format:
# *** Voting SPO epoch=EpochNo 508, action=GovActionId {gaidTxId = TxId {unTxId = SafeHash "15f82a365bdee483a4b03873a40d3829cc88c048ff3703e11bd01dd9e035c916"}, gaidGovActionIx = GovActionIx {unGovActionIx = 0}}; yesStake=39185410761088; totalActiveStake=22610003947264705, abstainStake=22153509629518900; (y,n,a,nv,df)[(KeyHash {unKeyHash = "ffffb02a48c007d4531ef7c38a6e354c013d47f9eb4862ac463d553d"},264534147584,(0,0,0,264534147584,0)),(KeyHash {unKeyHash = "58475ab22132ee593d295574d64064307b3338d4525eaaba11ce193f"},235700645897,(0,0,0,235700645897,0)),(KeyHash {unKeyHash = "581f191fbda6c996fc6734b4bfae0b6c274c84660570da8074aeeec5"},0,(0,0,0,0,0)),(KeyHash {unKeyHash = "5808e8716e8b968c3043fc9bdc21db652c916ce97ffaddfe891db16c"},173785560841,(0,0,0,173785560841,0)),(KeyHash {unKeyHash = "57eb48cdf25980039f087207af09fafb4970018e322d37c11c9a2496"},48973943221087,(0,0,0,48973943221087,0)),(KeyHash {unKeyHash = "57e0985d50391676b06b2ecf640d03d59735cd0c9a99d6e9e6f805eb"},9074587323449,(0,0,0,9074587323449,0)),(KeyHash {unKeyHash = "57b01d7f3a656f0ad22bb3a726eb9d2a71bddfc4a0b15a88dea2882b"},1442483670,(0,0,0,1442483670,0)),
# Note that the number of "KeyHash" records in one line can be different, the number in brackets are the following: (yes,no,abstain,no_vote,defaulted,ignored)
# They should be aggregated by epoch, action (gaidTxId and gaidGovActionIx) -- and all addition info (stake numbers) should also be recorded
# If there are two records with same epoch, action -- but different additional info and different set of keyhash records -- it should be printed.

import re
from collections import defaultdict
from typing import List, Dict, Any

# Returns (type,hash,stake,[y,n,a,nv,df,ig])
def match_keyhash_tuple(s: str):
    # Примеры: (KeyHash {unKeyHash = "ffffb02a..."},264534147584,(0,0,0,264534147584,0))
    # -- (DRepKeyHash {unKeyHash = "..."},100,(0,0,0,100,0,0))
    # -- (DRepScriptHash {ScriptHash "..."},100,(0,0,0,100,0,0))
    g = re.match(
        r'\(?KeyHash \{unKeyHash = "([0-9a-f]+)"\},(\d+),\((\d+),(\d+),(\d+),(\d+),(\d+)\)\)?', s)
    if g:
        return ("SPO", g.group(1), int(g.group(2)), [int(v) for v in g.groups()[2:]])  # (y,n,a,nv,df)

    g = re.match(
        r'\(?(DRepScriptHash|DRepKeyHash)[A-Za-z =\{\(]+"([0-9a-f]+)"[ )}]*,(\d+),\((\d+),(\d+),(\d+),(\d+),(\d+),(\d+)\)\)?', s)
    if g:
        type = ""
        if g.group(1)[0:5] == "DRepK":
            type = "DRK"
        elif g.group(1)[0:5] == "DRepS":
            type = "DRS"
        else:
            return None

        return (type, g.group(2), int(g.group(3)), [int(v) for v in g.groups()[3:]])  # (y,n,a,nv,df,ig)
    else:
        return None


def parse_keyhash_tuple(s: str):
    tuple = match_keyhash_tuple(s)

    if not tuple:
        print("incorrect keyhash tuple: ", s)
        return None

    (type, key, s_stake, votes) = tuple

    stake = int(s_stake)
    if votes[0] != 0:
        vote = 'Yes'
    elif votes[1] != 0:
        vote = 'No'
    elif votes[2] != 0:
        vote = 'Abstain'
    else:
        # Not voted
        return None

    if votes[4] != 0:
        vote = 'Default:' + vote

    if len(votes) > 5 and votes[5] != 0:
        # Not voted/invalid
        return None

    stake_sum = sum(votes[0:4])
    if stake != stake_sum:
        print("stake does not match votes in tuple: ", stake, stake_sum, s)
        return None

    df = votes[4]
    if vote != '':
        return ((type,key), (vote, stake))
    else:
        return ((type,key), ('', stake))

def parse_line(type: str, line: str):
    if ("Voting "+type) not in line:
        return None

    # Извлекаем epoch, gaidTxId, gaidGovActionIx, yesStake, totalActiveStake, abstainStake
    epoch_match = re.search(r'epoch=EpochNo (\d+)', line)
    txid_match = re.search(r'gaidTxId = TxId \{unTxId = SafeHash "([0-9a-f]+)"\}', line)
    actionix_match = re.search(r'gaidGovActionIx = GovActionIx \{unGovActionIx = (\d+)\}', line)
    yes_stake_match = re.search(r'yesStake=(\d+)', line)
    total_active_stake_match = re.search(r'totalActiveStake=(\d+)', line)
    abstain_stake_match = re.search(r'abstainStake=(\d+)', line)
    without_abstain_stake_match = re.search(r'totalExclAbstain=(\d+)', line)

    if not (epoch_match and txid_match and actionix_match):
        return None

    epoch = int(epoch_match.group(1))
    txid = txid_match.group(1)
    actionix = int(actionix_match.group(1))
    yes_stake = int(yes_stake_match.group(1)) if yes_stake_match else -1
    total_active_stake = int(total_active_stake_match.group(1)) if total_active_stake_match else -1
    abstain_stake = int(abstain_stake_match.group(1)) if abstain_stake_match else -1
    without_abstain_stake = int(without_abstain_stake_match.group(1)) if without_abstain_stake_match else -1

    #if type == "DRep":
    #    print(f"Parsing {type}: {epoch}, {txid}, {actionix}, {yes_stake}, {total_active_stake}, {abstain_stake}")

    # Извлекаем все кортежи KeyHash
    keyhashes = {}
    keyhashes_part = re.search(r'\[(.*)\]', line)
    if keyhashes_part:
        tuples_str = keyhashes_part.group(1)
        if tuples_str.startswith("(drep,stake,(y,n,a,nv,df,ig))]=["):
            tuples_str = tuples_str[len("(drep,stake,(y,n,a,nv,df,ig))]=["):]

        #print(tuples_str)
        tuples = tuples_str.split("),(")

        #tuples = re.findall(r'\(KeyHash|DRepScriptHash|DRepKeyHash [ A-Za-z"={}()0-9]+,\d+,\([0-9,]+\)\)', tuples_str)
        for t in tuples:
            if re.match(r'.*(DRepAlwaysAbstain|DRepAlwaysNoConfidence).*', t): continue

            parsed = parse_keyhash_tuple(t)
            if parsed:
                (key,value) = parsed
                keyhashes[key] = value

    return {
        "type": type,
        "epoch": epoch,
        "txid": txid,
        "actionix": actionix,
        "yes_stake": yes_stake,
        "total_active_stake": total_active_stake,
        "abstain_stake": abstain_stake,
        "without_abstain_stake": without_abstain_stake,
        "keyhashes": keyhashes,
        "raw": line.strip()
    }

# Line for DReps
# *** Voting DRep epoch=EpochNo 509action=GovActionId {gaidTxId = TxId {unTxId = SafeHash "15f82a365bdee483a4b03873a40d3829cc88c048ff3703e11bd01dd9e035c916"}, gaidGovActionIx = GovActionIx {unGovActionIx = 0}}; yesStake=4761426443345, totalExclAbstain=214843646653350, [(drep,stake,(y,n,a,nv,df,ig))]=[(DRepAlwaysNoConfidence,3281041063392,(0,3281041063392,0,0,1,0)),(DRepAlwaysAbstain,59620765920228,(0,0,59620765920228,0,1,0)),(DRepKeyHash (KeyHash {unKeyHash = "ff1e7f74f11fc382366c8155ac1f54f1f7cfb633c60df47d1f41c513"}),143995584122,(0,0,0,143995584122,0,0)),(DRepKeyHash (KeyHash {unKeyHash = "fdec0e7b970169151874a50e0f22f41fe95dd722eb0e1a11364095e2"}),2199109823,(2199109823,0,0,0,0,0)),(DRepKeyHash (KeyHash {unKeyHash = "fcc1946fe92b7f27a8b21d6639bffc72be07157b2745ef204d7467c0"}),170776300140,(0,0,0,170776300140,0,0)),(DRepKeyHash (KeyHash {unKeyHash = "fbb04f0ce09437f576719c8c4e82da0db7964f42be8c0c3e6b409827"}),177353002608,(0,0,0,177353002608,0,0)),
# Note, there are four types of individual votes. Two of them specify exact hash for the voters:
# 1. DRepKeyHash (KeyHash {unKeyHash ... })
# 2. DRepScriptHash (ScriptHash {...})
# and two more types, which present exactly once in the list, these are default votes:
# 3. DRepAlwaysAbstain
# 4. DRepAlwaysNoConfidence
# Implement functions, similar to parse_line and parse_keyhash_tuple for this kind of line



def aggregate_records(records: List[Dict[str, Any]]):
    # Группируем по (epoch, txid, actionix)
    grouped = {}
    for rec in records:
        key = (rec["type"], rec["epoch"], rec["txid"], rec["actionix"])
        if key in grouped:
            #print(f"*** Duplicate key: {key} => {grouped[key]}")

            old = grouped[key]
            #print (f"*** {rec['keyhashes']}")
            #print (f"*** {old['keyhashes']}")

            if old != rec:
                #print(f"Warning: duplicate data for {key}")
                #print("new: ",len(rec['keyhashes']))
                #print("old: ",len(old['keyhashes']))

                for k in rec['keyhashes'].keys():
                    if k not in old['keyhashes']: print("New record: ", k, rec['keyhashes'][k])
                    elif old['keyhashes'][k] != rec['keyhashes'][k]:
                        print("Difference in record: ", k, "old: ", old['keyhashes'][k], "new: ", rec['keyhashes'][k])

                for k in old['keyhashes'].keys():
                    if k not in rec: print("Old record: ", k, old['keyhashes'][k])
        else:
            #print(f"*** Grouped({key}) => {rec}")
            grouped[key] = rec
    return grouped

#            fn apply_votes_pattern(&self, action_id: &GovActionId, new_epoch: u64) -> Option<String> {
#                let pattern = self.verify_votes_files.as_ref()?;
#                let tx_hash = hex::encode(action_id.transaction_id)[0..8].to_string();
#                let act_id = format!("{tx_hash}_{}", action_id.action_index);
#                let applied = pattern.replace("{action_id}", &act_id)
#                    .replace("{epoch}", &new_epoch.to_string());
#                Some(applied)
#            }

def apply_pattern(hash: str, idx: int, new_epoch: int, pattern: str) -> str:
    """
    Формирует строку пути/имени файла по шаблону, подставляя значения action_id и epoch.
    action_id: dict с ключами 'transaction_id' и 'action_index'
    new_epoch: номер эпохи (int)
    pattern: строка-шаблон с {action_id} и {epoch}
    """
    tx_hash = hash[:8]
    act_id = f"{tx_hash}_{idx}"
    applied = pattern.replace("{action_id}", act_id).replace("{epoch}", str(new_epoch))
    return applied

def epoch_change(output_pattern, records, general_stats):
    grouped = aggregate_records(records)
    joined = {}
    for (type,epoch,hash,idx), recs in grouped.items():
        key = (epoch,hash,idx)
        if key not in joined:
            joined[key] = {}

        for (voter,vote) in recs['keyhashes'].items():
            if voter in joined[key]:
                print(f"Error: common voter for different types: {key}, pool {pool}")
            joined[key][voter] = vote

        # Sanity check:
        total_yes = 0
        for (voter,(vote,stake)) in recs['keyhashes'].items():
            if vote == "Yes":
                total_yes += stake
        if recs['yes_stake'] != total_yes:
            print(f"Sanity check fail for {type},{key}: {total_yes} not sums to yes stake from Haskell: {recs['yes_stake']}")

        general_stats[(epoch,hash,idx,type)] = (recs['yes_stake'], recs['total_active_stake'], recs['abstain_stake'], recs['without_abstain_stake'])

    for (epoch,hash,idx), recs in joined.items():
        filename = apply_pattern(hash, idx, epoch, output_pattern)

        f = open(filename, "w")
        f.write('"type","voter-hash","vote","voting-stake"\n')
        for (pool_type,pool_id), (vote, stake) in recs.items():
            if vote=='Default' or vote=='NoVote':
                continue

            f.write(f'"{pool_type}","{pool_id}","{vote}",{stake}\n')

def make_stats(stats_file, general_stats):
    fo = open(stats_file, "wt")
    fo.write('"epoch","tx","tx-idx","committee-yes","committee-excl-abstain","DRep-yes","DRep-excl-abstain","SPO-yes","SPO-active","SPO-abstain"\n')
    gstats = {}
    for (epoch,tx,idx,type),(ys,ts,abss,ws) in general_stats.items():
        old = {}
        if (epoch,tx,idx) in gstats:
            old = gstats[(epoch,tx,idx)]
        old[type] = (ys,ts,abss,ws)
        gstats[(epoch,tx,idx)] = old

    for (epoch,tx,idx),v in gstats.items():
        h = '%d,"%s",%d,0,0,' % (epoch,tx,idx)
        if 'DRep' in v:
            (ys,ts,abss,ws) = v['DRep']
            h += '%d,%d,' % (ys,ws)
        else:
            h += '0,0,'

        if 'SPO' in v:
            (ys,ts,abss,ws) = v['SPO']
            h += '%d,%d,%d' % (ys,ts,abss)
        else:
            h += '0,0,0'

        fo.write(h + '\n')

def main():
    import sys

    if len(sys.argv) <= 3:
        print("Usage: python convert_haskell_governance.py [input_file] [output_pattern] [stat-file]")
        print("Example: python convert_haskell_governance.py input.txt 'output_{action_id}_{epoch}.csv' 'stats.csv'")
        sys.exit(1)

    input_file = sys.argv[1]
    output_pattern = sys.argv[2]
    stats_file = sys.argv[3]

    f = open(input_file, "r")
    records = []
    lineno = 0
    epoch = 0
    general_stats = {}

    for line in f:
        lineno += 1
        if line.startswith('#'): continue

        line = line.strip()
        if not line.startswith('*** Voting'): continue

        if re.match(r'\*\*\* Voting.*\*\*\*', line): continue

        parsed = parse_line("SPO", line)
        if parsed:
            #if parsed['epoch'] > 520: 
            #    print("Parsed all epochs")
            #    break
            if epoch < parsed['epoch']:
                epoch_change(output_pattern, records, general_stats)
                print(f"New epoch {parsed['epoch']}")
                records = []
                epoch = parsed['epoch']
            elif epoch > parsed['epoch']:
                print(f"WARN: Old epoch {parsed['epoch']} met")
            records.append(parsed)
            continue

        parsed = parse_line("DRep", line)
        if parsed:
            if epoch < parsed['epoch']:
                epoch_change(output_pattern, records, general_stats)
                print(f"New epoch {parsed['epoch']}")
                records = []
                epoch = parsed['epoch']
            elif epoch > parsed['epoch']:
                print(f"WARN: Old epoch {parsed['epoch']} met")
            records.append(parsed)
            continue

        if line.startswith("*** Voting committee"):
            continue

        print(f"Failed to parse line {lineno}, {line}")

    make_stats(stats_file, general_stats)

if __name__ == "__main__":
    main()
