.PHONY: dev release test clean check fix

dev:
	cargo build

release:
	cargo build --release

test:
	cargo test

clean:
	cargo clean

check:
	cargo clippy -- -D warnings

fix:
	cargo clippy --fix --allow-dirty --allow-staged
