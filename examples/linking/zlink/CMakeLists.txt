cmake_minimum_required(VERSION 3.12)
project(MyProject)

# Find the zlib library
find_package(ZLIB REQUIRED)

# Include directories
include_directories(${ZLIB_INCLUDE_DIRS})

# Target executable
add_executable(zlink src/main.c)

# Link zlib to the executable
target_link_libraries(zlink ${ZLIB_LIBRARIES})

# Install the executable to bin/zlink
install(TARGETS zlink DESTINATION bin/)
