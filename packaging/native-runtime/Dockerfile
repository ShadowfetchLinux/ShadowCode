ARG BASE_IMAGE
FROM ${BASE_IMAGE}
COPY apk-packages.txt /build/apk-packages.txt
RUN xargs apk add --no-cache < /build/apk-packages.txt
# Release source archives also contain the retained dependency archives/patches.
COPY . /build/
WORKDIR /build
RUN bash build.sh
