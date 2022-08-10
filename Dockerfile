# Compile
FROM    rust:buster AS compiler

RUN 	apt update
RUN     apt -y install build-essential nodejs npm

WORKDIR chisel

COPY    . .
RUN     cargo build --release

# Run
FROM    debian:buster

COPY    --from=compiler /chisel/target/release/chisel /bin/chisel
COPY    --from=compiler /chisel/target/release/chiseld /bin/chiseld
COPY    --from=compiler /chisel/target/release/chiselc /bin/chiselc

EXPOSE  8080/tcp
