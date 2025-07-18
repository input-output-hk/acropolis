Stake Delta Filter module
=========================

The module subscribes to deltas topic, which gives info about money, 
deposited to accounts (positive delta) or retrieved from accounts (negative
deltas). The module then filters the topic and translates further only 
those deltas, which are bound to stake accounts.

There can be three types of addresses, and each type is processed in its own way:
* Byron addresses, they cannot be used in staking, therefore they are ignored.
* Base addresses (direct address of Staking account, not split into payment and
delegation parts), they are translated to output.
* Shelley addresses (split into payment and delegation parts). In this type of address,
payment part is ignored and the delegation part is translated further (if it is specified).
Enterprise addresses (which have empty delegation part) are not connected to any
staking, so they are ignored.

The staking part of Shelley address in turn may be specified directly (in this case
this info is forwarded to ouptut), or via a pointer. The pointers specify a block 
number, transaction and certificate index of the actual staking address for 
transaction. If the pointer is valid, then the pointer is substiuted with the 
corresponding stake account. If the pointer is invalid, then it is ignored 
(such pointers are allowed to address money, but are not allowed to participate in 
staking).

Pointer Cache
-------------

In order to correctly dereference pointers, one must have a track of all
possible accounts that could be referenced. This is done using pointer cache. 
This cache can be built "on the fly" (node parses all transactions and adds
that info into cache) or a precompiled hash can be be used instead.

The way of pointer cache work is determined by the following parameters in module
configuration .toml file:

* `cache-mode` (parameter is one of the strings):
  - `predefined`: tries to use predefined cache, collected by the module developers.
     Default behaviour, does not require any additional files. The predefined cache is collected for
     a fixed set of network ids only. If the network is unknown, then it fails.

     This mode is probably the best variant for an ordinal user because it is not allowed to make any
     pointers to Conway epoch stakes. Also, the authors believe that no new pointers may appear since 
     Conway epoch started, and only those pointers that were actually used in the blockchain up to 
     this moment can exist at any future moment.

     So, if the network has advanced to Conway epoch, then predefined cache can be collected
     in the way that allows to decode any possible pointer address.

  - `read`: tries to read file from disk. The file is taken from cache directory, 
     the file name is equal to the network name (in lower-case, with .json extension). If the file is absent, fails.

  - `write`: collects pointer cache on the fly, and writes the results into cache directory.
     The file name is equal to the network name (in lower-case, with .json extension).
     Along with cache, additional file (with .track.log extension) is collected,
     containing extensive info about all pointers that were used in the blockchain.

  - `write-if-absent`: tries to `read` cache, if reading fails, then behaves as `write`.

* `write-full-cache` (boolean value). In case of `true`, writes all stake addresses
    that can potentially used as pointers. In case of `false`, writes only actually used 
    addresses. 

    In the second mode info about pointers that were used but could not be
    resolved to real addresses is also written to the disk. This is done in order to remove unnecessary
    warnings about unresolved pointers in production environment.
