cmake_minimum_required(VERSION 3.15)
project(provider_example_cmake LANGUAGES C)
add_executable(hello-cmake main.c)
install(TARGETS hello-cmake RUNTIME DESTINATION bin)
