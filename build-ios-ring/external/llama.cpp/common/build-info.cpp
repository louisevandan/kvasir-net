#include "build-info.h"

#include <cstdio>
#include <string>

int LLAMA_BUILD_NUMBER = 7;
char const * LLAMA_COMMIT = "86a56fb";
char const * LLAMA_COMPILER = "AppleClang 17.0.0.17000604";
char const * LLAMA_BUILD_TARGET = "iOS ";

int llama_build_number(void) {
    return LLAMA_BUILD_NUMBER;
}

const char * llama_commit(void) {
    return LLAMA_COMMIT;
}

const char * llama_compiler(void) {
    return LLAMA_COMPILER;
}

const char * llama_build_target(void) {
    return LLAMA_BUILD_TARGET;
}

const char * llama_build_info(void) {
    static std::string s = "b" + std::to_string(LLAMA_BUILD_NUMBER) + "-" + LLAMA_COMMIT;
    return s.c_str();
}

void llama_print_build_info(void) {
    fprintf(stderr, "%s: build = %d (%s)\n",      __func__, llama_build_number(), llama_commit());
    fprintf(stderr, "%s: built with %s for %s\n", __func__, llama_compiler(), llama_build_target());
}
