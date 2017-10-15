comm
====

Primitives for inter-thread communication. See the documentation for a list of
features provided by this crate.

[Documentation](http://mahkoh.github.io/comm/doc/comm/)

### Comparison with the `std::sync` implementation

_   | `std::sync` | `comm`
----| :----: | :----:
Restricted by stability guarantees | ✔<sup>1</sup> | ✘
Users can use their own channels with `Select` | ✘ | ✔
`Select` has a safe interface | ✘ | ✔
`Select` can poll all channels in a vector without borrowing the vector | ✘ | ✔
`Select` can be used concurrently from multiple threads | ✘ | ✔
`Select` will be available in 1.0 | ✘ | ✔
Contains hot, experimental channels | ✘ | ✔
Extensively tested and optimized | ✔ | ✘
Web-Scale | ✘ | ✔<sup>2</sup>

<sup>1</sup>[Stability as a Deliverable](http://blog.rust-lang.org/2014/10/30/Stability.html)  
<sup>2</sup>Uses the epoll design.

In general: Channels in Rust don't need and don't have special compiler
support.  Therefore, channels in the official distribution have all the
restrictions that come with the stdlib without getting any benefits (unlike Go
channels which are tightly integrated with the language.)

### Usage

To use `comm`, first add this to your `Cargo.toml`:

```toml
[dependencies.comm]
git = "https://github.com/mahkoh/comm"
```

`comm` is currently not on [Crates.io](http://crates.io).

Then add this to your crate root:

```rust
extern crate comm;
```

### Bugs

There are some tests but, given the nature of multi-threaded code, some bugs
might only show up in production.

### License

MIT
