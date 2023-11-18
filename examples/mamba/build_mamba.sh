if [[ $PKG_NAME == "mamba" ]]; then
    cd mamba
    $PYTHON -m pip install . --no-deps -vv

    echo "Adding link to mamba into condabin";
    mkdir -p $PREFIX/condabin
    ln -s $PREFIX/bin/mamba $PREFIX/condabin/mamba

    exit 0
fi 

rm -rf build
mkdir build
cd build

export CXXFLAGS="${CXXFLAGS} -D_LIBCPP_DISABLE_AVAILABILITY=1"

if [[ $PKG_NAME == "libmamba" ]]; then
    cmake .. ${CMAKE_ARGS}              \
        -GNinja                         \
        -DCMAKE_INSTALL_PREFIX=$PREFIX  \
        -DCMAKE_PREFIX_PATH=$PREFIX     \
        -DBUILD_LIBMAMBA=ON             \
        -DBUILD_SHARED=ON               \
        -DBUILD_MAMBA_PACKAGE=ON
elif [[ $PKG_NAME == "libmambapy" ]]; then
    # TODO finds wrong python interpreter!!!!
    cmake .. ${CMAKE_ARGS}              \
        -GNinja                         \
        -DCMAKE_PREFIX_PATH=$PREFIX     \
        -DCMAKE_INSTALL_PREFIX=$PREFIX  \
        -DPython_EXECUTABLE=$PYTHON     \
        -DBUILD_LIBMAMBAPY=ON
fi

ninja

ninja install

if [[ $PKG_NAME == "libmambapy" ]]; then
    cd ../libmambapy
    rm -rf build
    $PYTHON -m pip install . --no-deps -vv
    find libmambapy/bindings* -type f -print0 | xargs -0 rm -f --
fi