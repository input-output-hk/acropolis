# Acropolis 'omnibus' process

This process contains all the [modules](../../modules), communicating
over the in-memory message bus, which makes it very easy to test.  It
is not suggested this is the right way to package things for
production.

## How to run it

Use `./replayer --governance-collect` to collect data about governance,
which will be saved in `governance-logs` subdirectory of the executable 
directory.

Use `./replayer --governance-replay` to re-run the process on the same
data.

