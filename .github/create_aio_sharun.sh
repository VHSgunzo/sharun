#!/bin/sh
set -e

ARCH="$(uname -m)"

apk add bash file binutils patchelf findutils grep sed coreutils strace which wget

BINS="bash patchelf strip strace find file grep sed awk \
xargs kill rm cp ln mv sleep echo readlink chmod sort \
cut mkdir basename dirname uname wget"

BINS_PATHS=
for bin in $BINS
    do BINS_PATHS="$BINS_PATHS $(which "$bin")"
done

export WRAPPE="$PWD/wrappe"
wget "https://github.com/VHSgunzo/wrappe/releases/latest/download/wrappe-$ARCH" -O "$WRAPPE"
chmod +x "$WRAPPE"

SHARUN="$PWD/sharun-$ARCH" \
"$PWD/lib4bin" -k -o -c 22 -s -g $BINS_PATHS "$WRAPPE"

mv sharun "sharun-$ARCH-aio"
rm -f wrappe*
