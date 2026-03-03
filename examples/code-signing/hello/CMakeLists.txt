cmake_minimum_required(VERSION 3.12)
project(hello C)

add_executable(hello src/main.c)

# Build a shared library too, so we test signing both .exe/.dll and binary/.dylib
add_library(greet SHARED src/greet.c)
target_link_libraries(hello greet)

install(TARGETS hello DESTINATION bin/)
install(TARGETS greet
    RUNTIME DESTINATION bin/   # .dll on Windows
    LIBRARY DESTINATION lib/   # .so / .dylib on Unix
)
