FROM ubuntu:22.04

# If you need to add dependencies, don't forget to push the image to registry and bump the version
# in .github/workflows/rust.yml
RUN apt-get update &&\
    apt-get upgrade -y &&\
    apt-get install -y lld git curl jq build-essential pkg-config libssl-dev postgresql-client uuid-runtime zstd sudo &&\
    apt-get clean -y

RUN useradd -u 1000 ubuntu && echo "ubuntu ALL=NOPASSWD: ALL" > /etc/sudoers.d/sudo
