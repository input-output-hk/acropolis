# Protocol updates synchronisation

Currently only Conway governance is supported.

Protocol receives EnactState message for the current epoch from the Governance module,
updates the info, and sends EnactState message for the next epoch.

Other ProtocolParameters updates are also epoch-based.

Technical idea:
* governance collects proposal & votes up to "new epoch" block message;
* EnactState message is generated after that and translated into Parameters module with "new
epoch" message info.
* Parameters module updates the info and sends "new epoch" block message along with parameters.

So, the parameters are meant to be updated in the beginning of a new epoch, before any other
messages are checked and parsed.

E.g., a ususal user of the parameters moudle could look like this:

```let msg = some_random_queue.read();
if msg.block.new_epoch {
    let params = parameters_queue.read();
    assert(msg.block == params.block);
    update_parameters(params);
}
normal_processing(msg);```
