export RUST_LOG=agent=debug,channel=debug,common=debug,relay=debug,server=debug
export RUST_BACKTRACE=full
export APP_MODE=development
export SERVER_HOST=localhost
export SERVER_PORT=8080

server:
	cargo run --bin server -- start

client:
	SSH_PORT=12222 \
		cargo run --bin agent -- start

agent: client

web: wasm-dev
	npx http-server -c-1 -p 8083 channel

proxy: relay

relay:
	cargo run --bin relay start

dev: build

build:
	cargo build --all --all-features

prod:
	cargo build --all --release --all-features

arm:
	cross build --all --target aarch64-unknown-linux-musl --release

format:
	cargo fmt --all

db:
	docker run --rm -it --name phirepass-valkey -p 6379:6379 valkey/valkey:8.1.4

lint:
	cargo clippy --all --all-targets -- -D warnings -D dead_code

docker-server:
	docker build -f server/Dockerfile -t dimitrmok/phirepass-server:latest .
#	docker buildx build \
#		-t dimitrmok/phirepass-server:latest \
#		--platform linux/amd64,linux/arm64 \
#		-f server/Dockerfile \
#		--progress=plain \
#		--push \
#		.

docker-relay:
	docker build -f relay/Dockerfile -t dimitrmok/phirepass-relay:latest .
#	docker buildx build \
#		-t dimitrmok/phirepass-relay:latest \
#		--platform linux/amd64,linux/arm64 \
#		-f relay/Dockerfile \
#		--progress=plain \
#		--push \
#		.

docker-relay-buildx:
	docker buildx build \
		-t dimitrmok/phirepass-relay:latest \
		--platform linux/amd64,linux/arm64 \
		-f relay/Dockerfile \
		--progress=plain \
		--push \
		.

docker-agent:
	docker buildx build \
		-t dimitrmok/phirepass-agent:latest \
		--platform linux/amd64,linux/arm64 \
		-f agent/Dockerfile \
		--progress=plain \
		--push \
		.

docker-sandbox:
	docker buildx build \
        -t dimitrmok/phirepass-agent-sandbox:0.0.4 \
        --platform linux/amd64 \
        -f agent/Dockerfile.sandbox \
        --progress=plain \
        --push \
        .

wasm-dev:
	cd channel && \
        RUST_LOG=info wasm-pack build --target web \
            --out-name phirepass-channel \
            --out-dir pkg/debug

wasm-prod:
	cd channel && \
        RUST_LOG=info wasm-pack build --target web \
            --out-name phirepass-channel \
            --out-dir pkg/release \
            --release

wasm: wasm-dev wasm-prod

.PHONY: sshd
sshd:
	cd agent/sshd && \
    	docker build -t sshd-pass . && \
		docker run -it --rm -p 12222:22 \
            --name phirepass-sshd sshd-pass

.PHONY: server deamon client web relay build arm format db docker-server docker-agent docker-relay docker-relay-buildx wasm-dev wasm-prod
