ARG BASE_IMAGE
FROM ${BASE_IMAGE}
COPY apk-packages.txt /build/apk-packages.txt
RUN xargs apk add --no-cache < /build/apk-packages.txt
COPY upstream.tar.gz manifest.json alpine-sources.json isolated-extraction.patch build.sh Dockerfile /build/
WORKDIR /build
RUN bash build.sh
