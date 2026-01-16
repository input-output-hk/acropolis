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

Omnibus will now create a memory profile dump file called `jeprof.out` when run.

### **Build and run a release build of omnibus with debug symbols**
```
cargo run --profile=release-with-debug
```

### **Create an SVG of usage from the dump file**
```
jeprof \
  --nodecount=4096 \
  --maxdegree=32 \
  --svg \
  <path-to>/target/release-with-debug/acropolis_process_omnibus \
  jeprof.out \
  > heapdump.svg
```
