# Performance Profiling

## Linux perf

Linux perf has a couple of big advantages over a lot of the other options for
performance profiling:
- nothing special has to built into the executable, it just needs debug symbols
- it is relatively light weight

### Creating profile data

To create profile data, just run a release build with debug symbols of the
application with `perf`:

```
$ perf record -F99 --call-graph dwarf ../../target/release-with-debug/acropolis_process_omnibus
```

Terminating perf will stop the application and generate a `perf.data` file.
This can take quite a long time.

`-F99` reduces the size of the output by reducing the frequency of recorded
results. It's 99 rather 100 in order to avoid syncing up with regular
processing.
`--call-graph dwarf` tells perf to get call-graph information from debuginfo.

### Viewing the profile data

There are a lot of ways to do this, and there is a really good resource about
this here:
[Linux perf Profiler UIs](https://www.markhansen.co.nz/profiler-uis/)

Probably the simplest to use is the Firefox profiler. First you need to convert
the `perf.data` file to something it will understand:

```
$ perf script -F +pid > omnibus.perf
```

You can then go to the [Firefox Profiler](https://profiler.firefox.com/) and
simply load the `omnibus.perf` file to view it.

## Troubleshooting

### perf complains that Access to performance monitoring and observability operations is limited.

You can enable access to these temporarily (until next reboot?) with the
following command:

```
$ sudo sysctl -w kernel.perf_event_paranoid=1
```

There are more permanent methods to apply this that you can find with a web
search for `perf_event_paranoid`.

### perf complains that addr2line could not read first record

Whilst this sounds like a problem with the build debug information, it can be
caused by slow implementations of addr2line taking too long and perf timing out
on them. The following installs a fast addr2line:

```
$ cargo install addr2line --features="bin"
```
