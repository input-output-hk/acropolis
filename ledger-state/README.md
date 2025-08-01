# Canonical Ledger State

This directory contains the CDDL defintions for the canonical ledger state we are proposing. It will also contain the rust types generated by [cddl-codegen](https://github.com/dcSpark/cddl-codegen). Eventually, it may contain a library for interacting with and converting ledger state to a canonical representation.


The CDDL definitions for the canonical ledger state is inspired heavily from the [existing ledger era CDDL](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/conway/impl/cddl-files/conway.cddl) in the cardano node repository. It also relies heavily on the [cardano ledger specification](https://intersectmbo.github.io/formal-ledger-specifications/cardano-ledger.pdf) types, defined in Agda.


Refer to [RFC-8610](https://datatracker.ietf.org/doc/html/rfc8610) for more information on CDDL.
