cmake_minimum_required(VERSION 3.10)
project(inno C)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
# add_compile_options(-g -fsanitize=address)
# add_link_options(-fsanitize=address)

# Find PkgConfig
find_package(PkgConfig REQUIRED)

# Find Dependencies
pkg_check_modules(WAYLAND_CLIENT REQUIRED wayland-client)
pkg_check_modules(WAYLAND_PROTOCOLS REQUIRED wayland-protocols)
pkg_check_modules(CAIRO REQUIRED cairo)
pkg_check_modules(DBUS REQUIRED dbus-1)

# Wayland Code Generation
execute_process(
    COMMAND pkg-config --variable=pkgdatadir wayland-protocols
    OUTPUT_VARIABLE WAYLAND_PROTOCOLS_DIR
    OUTPUT_STRIP_TRAILING_WHITESPACE
)

find_program(WAYLAND_SCANNER wayland-scanner)

# Helper function to generate protocol code
function(generate_wayland_protocol xml_file source_file header_file)
    add_custom_command(
        OUTPUT ${source_file} ${header_file}
        COMMAND ${WAYLAND_SCANNER} private-code ${xml_file} ${source_file}
        COMMAND ${WAYLAND_SCANNER} client-header ${xml_file} ${header_file}
        DEPENDS ${xml_file}
    )
endfunction()

# Protocols
set(PROTOCOLS_DIR ${CMAKE_SOURCE_DIR}/protocols)
set(GENERATED_DIR ${CMAKE_BINARY_DIR}/generated)
file(MAKE_DIRECTORY ${GENERATED_DIR})

# XDG Shell
generate_wayland_protocol(
    ${PROTOCOLS_DIR}/xdg-shell.xml
    ${GENERATED_DIR}/xdg-shell-protocol.c
    ${GENERATED_DIR}/xdg-shell-client-protocol.h
)

# Layer Shell
generate_wayland_protocol(
    ${PROTOCOLS_DIR}/wlr-layer-shell-unstable-v1.xml
    ${GENERATED_DIR}/wlr-layer-shell-unstable-v1-protocol.c
    ${GENERATED_DIR}/wlr-layer-shell-unstable-v1-client-protocol.h
)

# Sources
set(SOURCES
    src/main.c
    src/config.c
    layer/render.c
    layer/cairo_text.c
    layer/wayland_init.c
    layer/layerhandler.c
    handler/dbus_handler.c
    ${GENERATED_DIR}/xdg-shell-protocol.c
    ${GENERATED_DIR}/wlr-layer-shell-unstable-v1-protocol.c
)

# Include Directories
include_directories(
    ${CMAKE_SOURCE_DIR}/layer
    ${CMAKE_SOURCE_DIR}/handler
    ${CMAKE_SOURCE_DIR}/src
    ${GENERATED_DIR}
    ${WAYLAND_CLIENT_INCLUDE_DIRS}
    ${CAIRO_INCLUDE_DIRS}
    ${DBUS_INCLUDE_DIRS}
)

# Executable
add_executable(inno ${SOURCES})

# Link Libraries
target_link_libraries(inno
    ${WAYLAND_CLIENT_LIBRARIES}
    ${CAIRO_LIBRARIES}
    ${DBUS_LIBRARIES}
    m 
    rt
)

# Install
install(TARGETS inno DESTINATION bin)
