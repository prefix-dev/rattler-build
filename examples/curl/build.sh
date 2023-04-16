#!/bin/bash

# Get an updated config.sub and config.guess
cp $BUILD_PREFIX/share/libtool/build-aux/config.* .

if [[ $target_platform =~ linux.* ]]; then
    USESSL="--with-openssl=${PREFIX}"
else
    USESSL="--with-secure-transport"
fi;

./configure \
    --prefix=${PREFIX} \
    --host=${HOST} \
    ${USESSL} \
    --with-ca-bundle=${PREFIX}/ssl/cacert.pem \
    --disable-static --enable-shared

make -j${CPU_COUNT} ${VERBOSE_AT}
make install

# Includes man pages and other miscellaneous.
rm -rf "${PREFIX}/share"
