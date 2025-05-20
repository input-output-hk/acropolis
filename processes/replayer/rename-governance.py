import glob
import json

json_files = [f for f in glob.glob('governance-logs/gov_00*.json')]

idx = 0
for j in sorted(json_files):
    f = open(j)
    data = json.load(f)
    #data["ReceivedTxs"]["sequence"]["number"] = idx
    #if idx > 1:
    #    data["ReceivedTxs"]["sequence"]["previous"] = idx-1
    #else:
    #    data["ReceivedTxs"]["sequence"]["previous"] = None
    f.close()

    f = open('governance-logs/%d.json' % idx, 'wt')
    json.dump(data, f)
    f.close()

    idx += 1