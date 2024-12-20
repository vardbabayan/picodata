FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive

# Please, keep the packages sorted. Thank you.
RUN set -e; \
    apt update -y && \
    apt install -y \
        autoconf \
        build-essential \
        cmake \
        curl \
        git \
        libcurl4-openssl-dev \
        libicu-dev \
        libldap2-dev \
        libreadline-dev \
        libsasl2-dev \
        libssl-dev \
        libtool \
        libunwind-dev \
        libyaml-dev \
        libzstd-dev \
        make \
        ncurses-dev \
        pkg-config \
        postgresql-client \
        python3 \
        python3-gevent \
        python3-pip \
        python3-six \
        python3-yaml \
        tzdata \
        unzip \
        wget \
        && apt-get clean all

RUN set -e; \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --profile default --default-toolchain 1.76.0
ENV PATH=/root/.cargo/bin:${PATH}

COPY docker-build-base/ci-log-section /usr/bin/ci-log-section

# install poetry
RUN curl -sSL https://install.python-poetry.org | python3 -
ENV PATH=/root/.local/bin:${PATH}
RUN poetry --version

# install nodejs
# https://github.com/nodesource/distributions#installation-scripts
RUN curl -SLO https://deb.nodesource.com/nsolid_setup_deb.sh && \
    chmod 755 nsolid_setup_deb.sh && \
    ./nsolid_setup_deb.sh 21 && \
    rm nsolid_setup_deb.sh && \
    apt-get install nodejs && \
    corepack enable && \
    apt-get clean all

# install glauth for ldap tests
RUN set -e; \
    cd /bin; \
    curl -L -o glauth https://github.com/glauth/glauth/releases/download/v2.3.0/glauth-linux-amd64; \
    chmod +x glauth;

# Timezone
RUN ln -fs /usr/share/zoneinfo/Europe/Moscow /etc/localtime
