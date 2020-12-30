# Snapshot Repeat

A simple VST plugin (without a GUI) which periodically captures a buffer and
then repeats it at a given speed until the end of the period.


## Parameters

- Period - samples, scaled linearly from `0 ..= 1` to `1 ..= 44_100 * 10`
- Capture length - samples, scaled linearly from `0 ..= 1` to `1 ..= 44_100 * 10`
- Playback rate - multiplier, scaled linearly from `0 ..= 1` to `0.01 ..= 100`


## Build

Plugin is built as a shared library using cargo directly:

```shell
cargo build --release
```

The file in `target/release/libsnapshot_repeat.{so,dll,lib}` can be directly
loaded by a VST plugin host.

I've only tested this with [Carla](https://kx.studio/Applications:Carla) on
linux so far, but the used VST library should allow it to work on any platform.
