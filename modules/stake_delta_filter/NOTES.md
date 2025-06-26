Module parameters
=================

Chain id
--------

Need to know chain id --- to properly work with cached pointer parameters.
Probably, system-wide parameters should be implemented and used.

Random notes
============

Pointer conversion issues
-------------------------

1. Pointer conversion: some pointers are meaningful, the most pointers are meaningless.

2. Docs says that there could be senseless pointers. However, they may function as
valid addresses for money tracking purposes: not for staking purposes.

3. The technical requirement for StakeAddressDelta module means (as it is possible to
understand) that only staking addresses (and their deltas) are interesting here. So,
probably, if address cannot be converted into real one, it may not be used for staking
purposes and should be skipped.

Open questions
--------------

1. How a network ID could be retrieved? (Main/Test for the beginning, full network ID 
at most).

2. Is it possible to have a pointer to the same slot? I believe not (one cannot know
the slot/block no before his transaction is processed).

If it would be possible, then we need proper order of incoming messages processing
(first certificates for a block, then transactions).
