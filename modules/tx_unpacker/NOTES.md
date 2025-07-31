# Random notes on tx

By default, Pallas unpacks transactions if they fit into Conway.
Alonzo nicely fits into Conway (for some reason), so additional Alonzo
information is lost.

To properly retrieve Alonzo-specific info (like Shelley parameter 
updates etc), one need to unpack it as Alonzo proper (using
`MultiEraTx::decode_for_era(traverse::Era::Alonzo, &raw_tx)`).
