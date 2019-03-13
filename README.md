# rust-pv

*rust-pv* is a simple, fast pv implementation in Rust by leverage tokio-fs.


# usage

```bash
rust-pv -b <block size> [<input file>] (default stdin)
```

## Examples

```bash
./target/debug/rust-pv -b 4096 -t 400 /dev/zero >/tmp/test.log
speed: 332 MBytes/s bs: 4096 elapsed: 2 s
```
