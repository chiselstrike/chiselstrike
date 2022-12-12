FROM ubuntu:22.04

# If you need to add dependencies, don't forget to push the image to registry and bump the version
# in .github/workflows/rust.yml
RUN apt-get update &&\
    apt-get upgrade -y &&\
    apt-get install -y lld git curl unzip jq build-essential pkg-config libssl-dev \
        postgresql-client uuid-runtime zstd sudo &&\
    apt-get clean -y

# Install deno
RUN curl -fsSL https://deno.land/x/install/install.sh | sh &&\
    mv /root/.deno/bin/deno /usr/bin/

RUN useradd -u 1000 ubuntu && echo "ubuntu ALL=NOPASSWD: ALL" > /etc/sudoers.d/sudo
