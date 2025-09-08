## Network

By default, messages are fetched from mainnet. However, other networks
are available (testnet, sanchonet). 

### Sanchonet
https://sancho.cardanoconnect.io/
Sanchonet has the following stats:
Epoch 750
Block Height 3203471

100 (400) blocks per minute
32030 (8000) minutes to sync
500 (150) hours to sync

## Protocol versions
SanchoNet has version 6.0 (Alonzo, 2nd version, after intra-era hardfork) in genesis.
In epoch 2 (or 3?) it upgrades to 8.0 (Babbage, Valentine HF).
It stays 8.0 till epoch 492, where it upgrades to 9.0 (Conway, ChangHF).

Version numbers are taken from CIP-0059:
https://github.com/cardano-foundation/CIPs/blob/master/CIP-0059/feature-table.md

## header `variant` field from Pallas header parser (chain-sync protocol)

It *seems*, that it's TipInfo from Haskell node. 
ouroboros-consensus-cardano/Ouroboros/Consensus/Cardano/Block.hs

```
        {-# COMPLETE TipInfoByron
                   , TipInfoShelley
                   , TipInfoAllegra
                   , TipInfoMary
                   , TipInfoAlonzo
                   , TipInfoBabbage
                   , TipInfoConway
          #-}
```

Numbers are given in another place:

```
        pattern TagByron   x =                   Z x
        pattern TagShelley x =                S (Z x)
        pattern TagAllegra x =             S (S (Z x))
        pattern TagMary    x =          S (S (S (Z x)))
        pattern TagAlonzo  x =       S (S (S (S (Z x))))
        pattern TagBabbage x =    S (S (S (S (S (Z x)))))
        pattern TagConway  x = S (S (S (S (S (S (Z x))))))
```

But the question needs additional research.

