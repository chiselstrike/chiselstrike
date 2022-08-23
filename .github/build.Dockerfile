FROM ubuntu:22.04

# If you need to add dependencies, don't forget to push the image to registry and bump the version
# in .github/workflows/rust.yml
RUN apt-get update &&\
    apt-get upgrade -y &&\
    apt-get install -y lld git curl jq build-essential pkg-config libssl-dev\
        postgresql-client uuid-runtime zstd &&\
    apt-get autoremove && apt-get clean -y

RUN git clone https://github.com/rui314/mold.git &&\
    cd mold &&\
    git checkout v1.4.1 &&\
    ./install-build-deps.sh &&\
    make -j$(nproc) &&\
    make install &&\
    cd .. &&\
    rm -rf mold &&\
    apt-get autoremove && apt-get clean -y
