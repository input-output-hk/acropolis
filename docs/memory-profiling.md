# Memory Profiling
## Jemalloc

### **Enable profiling**
Setting this environment variable in the env omnibus is running will turn on profiling:
```
export _RJEM_MALLOC_CONF=prof:true
```

In the omnibus.toml, configure a profile dump to happen at a given epoch/break
(same kind of setting as the pause/stop configurations):
```
[module.mithril-snapshot-fetcher]
profile = "epoch:300"
```

Omnibus will now create a memory profile dump file called `memory-epoch-300.jeprof` when run.

Omnibus can also be configured to produce jeprof profiles at given intervals, e.g.:
```
[module.mithril-snapshot-fetcher]
profile = "every-nth-epoch:5"
```
This can be useful to compare what memory allocations have happened between two
points. This can be done using the `-b` option of jeprof to provide a base
profile as a comparison point.

### **Build and run a release build of omnibus with debug symbols**
```
cargo run --profile=release-with-debug
```

### **Create an SVG of usage from the dump file**
```
jeprof \
  --lines \
  --nodecount=4096 \
  --maxdegree=32 \
  --svg \
  <path-to>/target/release-with-debug/acropolis_process_omnibus \
  memory-epoch-300.jeprof \
  > heapdump.svg
```

### **Create an SVG of differences between two profile files**
```
jeprof \
  --lines \
  --nodecount=4096 \
  --maxdegree=32 \
  --svg \
  <path-to>/target/release-with-debug/acropolis_process_omnibus \
  -b memory-epoch-295.jeprof \
  memory-epoch-300.jeprof \
  > heapdiff.svg
```
