.PHONY: build run fmt clippy test

RUSTFLAGS ?= -D warnings

build:
	cargo build --release

run:
	cargo run --

fmt:
	cargo fmt --all

clippy:
	cargo clippy --all-features -- -D warnings

test:
	cargo test --all-features

migrate:
	cargo run -- migrate

sync:
	cargo run -- sync --steamid $(STEAMID)
