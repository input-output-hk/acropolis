# ProtocolParams updates synchronisation

Let us suppose that epoch N is changed by epoch N+1.

Parameters module receives EnactState message for epoch N (in the beginning of epoch N+1, the
message has BlockInfo of the first block in epoch N+1) from the Governance module.
It updates the info, and sends ProtocolParams message for the next epoch (it's labelled
with the same BlockInfo --- of the first block in epoch N+1).

Only messages at epoch boundaries are acceptable for EnactState messages and Governance messages.

E.g., a ususal user of the parameters moudle could look like this:

```let msg = some_random_queue.read();
if msg.block.new_epoch {
    let params = parameters_queue.read();
    assert(msg.block == params.block);
    update_parameters(params);
}
normal_processing(msg);```

# EnactState

Enact state is loosely related to the structure from Ledger technical description. It consists
of a series of elementary events.
