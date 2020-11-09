# Rust Parsing Exercise

This repository is a fork of TezEdge with Nom as a deserializer for
some of Tezos messages.

## Building Example

From `tezos/messages` directory, run the following command:

```
cargo build
```

## Running Unit Tests

From `tezos/messages` directory, run the following command:

```
cargo test
```

There are new tests that use Nom deserialization
`can_deserialize_nom_...`. All of them except one are just copied from
existing tests with the only modification on decoding method. The last
one, `can_deserialize_nom_operations_for_blocks_zig_zag`, is a new one
and written in order to test new non-recursive implementation of
`Path` decoding.

## Running Benchmarks

From `tezos/messages` directory, run the following command:

```
cargo bench
```

Tests named like `serde_...` use `Serde` for deserialization. Tests
named `nom_...` use new Nom-based deserialization.

