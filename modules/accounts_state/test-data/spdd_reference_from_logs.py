import re

check_acropolis = False

def decode_epoch(ln,tested_epoch):
    active = 0

    stakes_hs = {}

    for a in ln.split('),('):
        g = re.match(r'.*unKeyHash = \"([0-9a-f]+)\".*,Coin (\d+).*', a+')')
        if g:
            stakes_hs[g.group(1)] = int(g.group(2))
            active += int(g.group(2))

    print('active_hs =', active)

    fo = open('spdd.mainnet.%d.csv' % tested_epoch, 'wt')
    fo.write('pool_id,amount // Reference Mainnet SPDD distribution at the Epoch %d end = Epoch %d Go\n' 
             % (tested_epoch, tested_epoch + 3))
    for pool_id in stakes_hs.keys():
        if stakes_hs[pool_id] > 0:
            fo.write('%s,%d\n' % (pool_id, stakes_hs[pool_id]))
    fo.close()

starting_epoch = 212

f = open('../log8-1','rt')
for ln in f:
    starting_line = "**** startStep computation: epoch=EpochNo %d, stake=Stake {unStake = fromList [" % (starting_epoch+3)
    if ln.startswith(starting_line):
        stake_per_pool_start = ln.find('stakePerPool=fromList [')
        stake_per_pool_ending = ln.find(']', stake_per_pool_start)
        stake_per_pool = ln[stake_per_pool_start:stake_per_pool_ending]
        decode_epoch(stake_per_pool,starting_epoch)
        starting_epoch += 1
