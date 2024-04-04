@echo ON

if /I "%PKG_NAME%" == "mamba" (
	cd mamba
	%PYTHON% -m pip install . --no-deps -vv
	exit 0
)

rmdir /Q /S build
mkdir build
cd build

rem most likely don't needed on Windows, just for OSX
rem set "CXXFLAGS=%CXXFLAGS% /D_LIBCPP_DISABLE_AVAILABILITY=1"

if /I "%PKG_NAME%" == "libmamba" (
	cmake .. ^
	    %CMAKE_ARGS% ^
		-GNinja ^
		-DCMAKE_INSTALL_PREFIX=%LIBRARY_PREFIX% ^
		-DCMAKE_PREFIX_PATH=%PREFIX% ^
		-DBUILD_LIBMAMBA=ON ^
		-DBUILD_SHARED=ON  ^
		-DBUILD_MAMBA_PACKAGE=ON
)
if /I "%PKG_NAME%" == "libmambapy" (
	cmake .. ^
	    %CMAKE_ARGS% ^
		-GNinja ^
		-DCMAKE_INSTALL_PREFIX=%LIBRARY_PREFIX% ^
		-DCMAKE_PREFIX_PATH=%PREFIX% ^
                -DPython_EXECUTABLE=%PYTHON% ^
		-DBUILD_LIBMAMBAPY=ON
)
if errorlevel 1 exit 1

ninja
if errorlevel 1 exit 1

ninja install
if errorlevel 1 exit 1

if /I "%PKG_NAME%" == "libmambapy" (
	cd ../libmambapy
	rmdir /Q /S build
	%PYTHON% -m pip install . --no-deps -vv
	del *.pyc /a /s
)
