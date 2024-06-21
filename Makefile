.PHONY: build-debug build-release

build-debug:
	cargo build --target x86_64-unknown-linux-gnu
	du -sh target/x86_64-unknown-linux-gnu/debug/rook
build-release:
	cargo build --target x86_64-unknown-linux-gnu --release
	du -sh target/x86_64-unknown-linux-gnu/release/rook
