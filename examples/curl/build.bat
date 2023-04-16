mkdir build

cmake -GNinja ^
      -DCMAKE_BUILD_TYPE=Release ^
      -DBUILD_SHARED_LIBS=ON ^
      -DCMAKE_INSTALL_PREFIX=%LIBRARY_PREFIX% ^
      -DCMAKE_PREFIX_PATH=%LIBRARY_PREFIX% ^
      -DCURL_USE_SCHANNEL=ON ^
      -DCURL_USE_LIBSSH2=OFF ^
      -DUSE_ZLIB=ON ^
      -DENABLE_UNICODE=ON ^
      %SRC_DIR%

IF %ERRORLEVEL% NEQ 0 exit 1

ninja install --verbose
