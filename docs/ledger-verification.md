# Ledger verification

1. Blocks and transactions verification requires many steps, and some of the steps' outcomes
may become clear only in the middle of the process. Therefore, in order to check a transaction/
block one should repeat the whole process of block application.
Therefore, an idea of combined verification/application process is considered, we call it
ledger verification.

2. The ledger always checks transactions for all errors and performs all necessary checks.
But the reaction to the errors can be different.

# Two types of reaction

We have basically two variants of behaviour if the error in application/verification occurred:

1. Print an error message. This happens when an incorrect block apply happened, which probably
   broke the state. We can only hope that further blockchain rollbacks will correct it.

2. Send a ValidationStatus::NoGo message via corresponding channel (each module has a validation
   outcome channel). These channels are listened by Consensus module, and if at least one of them
   sent 'NoGo' outcome, then the block is rejected.

   If the block was validated as 'NoGo', the ledger state should remain intact.

# How it works

1. Blocks and transactions can appear in Acropolis from two sources:
   * Mithril/other trusted source --- blocks, already accepted by the blockchain.
     If everything is ok, then the block is applied, internal structures updated, and next block
     is processed.
     If something is not correct, then the whole blockchain is broken, and outside intervention
     is required.

   * Mempool or consensus blocks --- proposals for the blockchain. If the block/transaction is not 
     successfully verified, then it could be refused.
     If block is verified, it may be included into chain as mutable (optimistic version),
     or checked in two phases: first verified, and then processed as trusted block (pessimistic).

2. `BlockInfo` data structure contains `BlockIntent` field, which (along with block number) control
   this process of block and transaction application and verification.

## `BlockIntent` field

1. There are three different ways of block processing (specified in `BlockIntent`):
   * `Validate`: check block and send ValidateState::Go (if block is valid) or NoGo (block is invalid).
     Internal state should not change.
   * `Apply`: apply block, print error if it's incorrect (internal structures correctness is not 
     guaranteed, behaviour after such application is undefined).
   * `ValidateAndApply`: check block and apply it if block is correct, send error if it's incorrect.
     Internal state should not change if the block is incorrect.

   The `Apply` variant is easier to implement, since it does not require integrity of data structures
   if incorrect transaction/block is applied. 

2. Application can be initiated by Mithril/Upstream fetcher/Peer network interface modules: modules that
   trust the sources. It has no natural reaction to errors.

3. Validation is initiated by Consensus module. That module check block candidates one by one, and it
   handles error reaction by rejecting the block candidate and trying the next one.

## Block number and rollbacks

1. `BlockInfo` keeps track of the current block number. Blocks are numbered sequentially. So if the
   number equal to the previous one (or smaller than it), then Rollback takes place (all info from
   blocks with this or greater number should be deleted, and new attempt to apply block is done).
   In another words, applying of block N may be possible only if module state is actual for block N-1.

2. So, if the block applied unsuccessfully (and internal structures are broken), the situation 
   can be corrected by rolling back to last correct block and applying different (correct) block 
   after it.

   However, after unsucessful application and before successful rollback the state of the node is
   incorrect.

## Mulit-module ledger specifics

Ledger is split into several modules, so it gives additional challenges to the validation process.

1. And if one module verified the block, then next modules in chain of messages may still reject it. 
   We should guarantee that the state of the ledger would not become inconsistent in this situation.

2. In case some block is invalid, we may skip sending messages to further dependent modules.
   This makes synchronisation harder. Consensus module should check not only message numbers,
   but also block data (hash, etc), and skip all replies that do not correspond to current verification.

   Instead, it should wait either for all 'Go' (from all modules), or for at least one 'NoGo',
   and do not wait for further messages.
