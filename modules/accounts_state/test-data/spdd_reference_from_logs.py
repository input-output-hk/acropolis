import re
import sys

check_acropolis = False

def decode_epoch(ln,tested_epoch,stakes_hs):
    active = 0

    for a in ln.split('),('):
        g = re.match(r'.*unKeyHash = \"([0-9a-f]+)\".*,Coin (\d+).*', a+')')
        if g:
            if g.group(1) not in stakes_hs:
                stakes_hs[g.group(1)] = {}

            stakes_hs[g.group(1)][tested_epoch] = int(g.group(2))
            active += int(g.group(2))

    print('active_hs =', active)

    #fo = open('spdd.mainnet.%d.csv' % tested_epoch, 'wt')
    #fo.write('pool_id,amount // Reference Mainnet SPDD distribution at the Epoch %d end = Epoch %d Go\n' 
    #         % (tested_epoch, tested_epoch + 3))
    #for pool_id in stakes_hs.keys():
    #    if stakes_hs[pool_id] > 0:
    #        fo.write('%s,%d\n' % (pool_id, stakes_hs[pool_id]))
    #fo.close()

def write_grouped(grouped_epochs,emin,emax):
    epochs_range = '%d-%d' % (emin,emax)
    if emin == emax:
        epochs_range = '%d' % emin

    fo = open('spdd.mainnet.%s.csv' % epochs_range, 'wt')
    ahdr = ['amount-%d' % e for e in range (emin,emax+1)]
    fo.write('pool_id,%s // Reference Mainnet SPDD distribution at the Epoch %d..=%d end = Epoch %d..=%d Go\n' 
             % (','.join(ahdr), emin, emax, emin + 3, emax + 3))
    for pool_id in grouped_epochs.keys():
        #if grouped_epochs[pool_id]:
        data = ""
        for e in range(emin,emax+1):
            if e in grouped_epochs[pool_id]:
                data += ",%d" % grouped_epochs[pool_id][e]
            else:
                data += ","
        fo.write('%s%s\n' % (pool_id, data))
    fo.close()

starting_epoch = 507
compact_epochs = 5

f = open(sys.argv[1],'rt')
grouped_epochs = {}
epoch = starting_epoch

for ln in f:
    starting_line = "**** startStep computation: epoch=EpochNo %d, stake=Stake {unStake = fromList [" % (epoch+3)
    if ln.startswith(starting_line):
        stake_per_pool_start = ln.find('stakePerPool=fromList [')
        stake_per_pool_ending = ln.find(']', stake_per_pool_start)
        stake_per_pool = ln[stake_per_pool_start:stake_per_pool_ending]
        if epoch >= starting_epoch+compact_epochs:
            write_grouped(grouped_epochs,starting_epoch,epoch-1)
            grouped_epochs = {}
            starting_epoch = epoch
        decode_epoch(stake_per_pool,epoch,grouped_epochs)
        epoch += 1

write_grouped(grouped_epochs,starting_epoch,epoch-1)
