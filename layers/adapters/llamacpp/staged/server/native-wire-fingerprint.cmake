# Native source identity, independent of checkout paths and CRLF conversion.
file(GLOB_RECURSE _p4_wire_sources CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_LIST_DIR}/src/*.cpp"
    "${CMAKE_CURRENT_LIST_DIR}/src/*.hpp"
    "${CMAKE_CURRENT_LIST_DIR}/src/*.inc")
list(FILTER _p4_wire_sources EXCLUDE REGEX "(_test\\.cpp|_tests\\.cpp)$")
list(APPEND _p4_wire_sources
    "${CMAKE_CURRENT_LIST_DIR}/CMakeLists.txt"
    "${CMAKE_CURRENT_LIST_FILE}")
list(SORT _p4_wire_sources)
set(_p4_wire_manifest "p4-native-wire-source-v1\n")
foreach(_p4_wire_file IN LISTS _p4_wire_sources)
    file(READ "${_p4_wire_file}" _p4_wire_text)
    string(REPLACE "\r\n" "\n" _p4_wire_text "${_p4_wire_text}")
    string(SHA256 _p4_wire_digest "${_p4_wire_text}")
    file(RELATIVE_PATH _p4_wire_name "${CMAKE_CURRENT_LIST_DIR}" "${_p4_wire_file}")
    string(APPEND _p4_wire_manifest "${_p4_wire_name}:${_p4_wire_digest}\n")
endforeach()
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS ${_p4_wire_sources})
string(SHA256 P4_STAGED_NATIVE_WIRE_SOURCE "${_p4_wire_manifest}")
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/native-wire-source.manifest" "${_p4_wire_manifest}")
