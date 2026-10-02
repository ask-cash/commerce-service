.PHONY: check fmt lint test run-server run-worker migrate

check: fmt lint test

fmt:
	cargo fmt --all --check

lint:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

migrate:
	cargo run -- migrate

run-server:
	cargo run -- server

run-worker:
	cargo run -- worker
