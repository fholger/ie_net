# IE::Net - Open source EarthNet lobby server

## Installation

Install `rust` and `cargo`: go to [rustup.rs](https://rustup.rs) and follow the instructions.

To build IE::Net, type
```
cargo build
```

To run IE::Net, type
```
cargo run
```

## Configuration

By default, IE::Net listens on all addresses at port 17171 (default EarthNet port).
You can change the listen address and port by passing a command line argument:
```
cargo run -- --bind 192.168.1.1:12345
```
