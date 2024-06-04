let cmake-args = [
    "-GNinja"
    "-DBUILD_TESTS=OFF"
    $"-DCMAKE_INSTALL_PREFIX=($env.LIBRARY_PREFIX)"
    $env.SRC_DIR
]

cmake $cmake-args
ninja install
