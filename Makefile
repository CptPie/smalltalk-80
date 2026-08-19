ARGS = RUSTFLAGS="-A unused"

build:
	$(ARGS) cargo build

test:
	$(ARGS) cargo test

run:
	$(ARGS) cargo run
