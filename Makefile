.PHONY: release-musl debug-musl

release-musl:
	docker-build/build.sh
debug-musl:
	cargo build --target=x86_64-unknown-linux-musl