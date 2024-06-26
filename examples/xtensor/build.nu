let prefix = if $nu.os-info.name == "windows" { $env.LIBRARY_PREFIX }  else { $env.PREFIX }
let cmake_args = [
    "-GNinja"
    "-DBUILD_TESTS=OFF"
    $"-DCMAKE_INSTALL_PREFIX=($prefix)"
    $env.SRC_DIR
]

cmake ...$cmake_args
ninja install
