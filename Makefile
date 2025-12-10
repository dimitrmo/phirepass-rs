export RUST_LOG=info

server:
	cargo run --bin server -- start

client:
	SSH_USER=$(USER) \
		cargo run --bin daemon -- start

daemon: client

web:
	cargo run --bin web

build:
	cargo build --all

prod:
	cargo build --all --release

arm:
	cross build --all --target aarch64-unknown-linux-musl --release

format:
	cargo fmt --all

db:
	docker run --rm -it --name phirepass-valkey -p 6379:6379 valkey/valkey:9

docker:
	docker buildx build \
		-t phirepass/daemon:latest \
		--platform linux/amd64,linux/arm64 \
		-f daemon/Dockerfile \
		--progress=plain \
		--push \
		.

.PHONY: server deamon client web build arm format db docker
