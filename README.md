# Tack

Tack is a **fork** of the [tac](https://github.com/neosmart/tac) crate.

Tack is a high-performance, simd-accelerated, cross-platform rewrite of the [GNU `tac` utility](https://www.gnu.org/software/coreutils/manual/html_node/tac-invocation.html#tac-invocation) from Coreutils, released under MIT/Apache-2.0 licenses. `tac` reads input from a file (or from `stdin`, but [see below](#implementation-notes)) and then prints it line-by-line backwards.

This `tac` implementation uses simd-acceleration for new line detection and utilizes memory-mapped files on all supported operating systems. It is additionally written in rust for maximum integrity and safety.

The MSRV is **1.70**.

## Who needs a faster `tac` anyway?

Good question. Try grepping through a multi-gigabyte web access log file in reverse chronological order (`tac --line-buffered access.log | grep foo`) and then get back to me.

## Usage

```bash
Usage: tac [OPTIONS] [FILE1..]
Write each FILE to standard output, last line first.
Reads from stdin if FILE is - or not specified.

Options:
  -h --help        Print this help text and exit
  -v --version     Print version and exit.
  --line-buffered  Always flush output after each line.
```

Tack reads lines from any combination of `stdin` and/or zero or more files and writes the lines to the output in reverse order.

### Example

```bash
$ echo -e "hello\nworld" | tac
world
hello
```

## Installation

Tack may be built installed via cargo, the rust package manager:

```bash
cargo install tac-k --locked
```

or installed with pre-built binaries via `cargo-binstall`:

```bash
cargo binstall tac-k --locked
```

## Implementation Notes

This implementation of `tac` uses SIMD instruction sets (AVX2, NEON) to accelerate the detection of new lines if available. The usage of memory-mapped files additionally boosts performance by avoiding slowdowns caused by context switches when reading from the input if speculative execution mitigations are enabled. It is significantly (2.55x if mitigations disabled, more otherwise) faster than the version of `tac` that ships with GNU Coreutils, in addition to being more liberally licensed.

**To obtain maximum performance:**

* Try not to pipe input into `tac`. e.g. instead of running `cat /usr/share/dict/words | tac`, run `tac /usr/share/dict/words` directly. Because `tac` by definition must reach the end-of-file before it can emit its input with the lines reversed, if you use `tac`'s `stdin` interface (e.g. `cat foo | tac`), it must buffer all `stdin` input before it can begin to process the results. `tac` will try to buffer in memory, but once it exceeds a certain high-water mark (currently 4 MiB), it switches to disk-based buffering (because it can't know how large the input is or if it will end up exceeding the available free memory).
* Always try to place `tac` at the _start_ of a pipeline where possible. Even if you can guarantee that the input to `tac` will not exceed the in-memory buffering limit (see above), `tac` is almost certainly faster than any other command in your pipeline, and if you are going to reverse the output, you will benefit most if you reverse it from the start, unless you are always going to run the command to completion. For example, instead of running `grep foo /var/log/nginx/access.log | tac`, run `tac /var/log/nginx/access.log | grep foo`. This will (significantly) reduce the amount of time/work before the first _n_ matches are reported (because the file is first quickly reversed then searched in the desired order, vs slowly searched in its entirety and only then are the results reversed).
* Use line-buffered output mode (`tac --line-buffered`) if tac is piping into another command rather than writing to the tty directly. This gives you "live" streaming of results and lets you terminate much sooner if you're only looking for the first _n_ matches. e.g. `tac --line-buffered access.log | grep foo` will print its first match much, much sooner than `tac access.log | grep foo` would.
* In the same vein, if you are chaining the output of _n_ utilities, make sure that all commands up to _n_ - 1 are all using line-buffered mode unless you don't care about latency and only care about throughput. For example, to print the first two matches for some grep pattern: `tac --line-buffered access.log | grep --line-buffered foo | head -n2`.

## License

Tack is licensed under either [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this crate by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

## Contribution

As an open source project, Tack would not exist without the tireless efforts of its various contributors - see [CONTRIBUTORS.md](CONTRIBUTORS.md) for full details.
