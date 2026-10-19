cmake_minimum_required(VERSION 3.22.1)
project(minkb_grammar CXX C)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

if(ANDROID_ABI STREQUAL "armeabi-v7a")
    # 32-bit phones get the keyboard without the AI; see grammar_stub.cpp.
    add_library(minkb_grammar SHARED grammar_stub.cpp)
else()
    # llama.cpp is pinned to a release tarball and checksum. Set LLAMA_CPP_SOURCE_DIR to use a local checkout.
    include(${CMAKE_CURRENT_LIST_DIR}/llama.cmake)
    add_library(minkb_grammar SHARED grammar_core.cpp grammar_jni.cpp)
    target_link_libraries(minkb_grammar PRIVATE llama)
    target_compile_options(minkb_grammar PRIVATE -O2)
endif()
target_link_libraries(minkb_grammar PRIVATE android log)
target_compile_options(minkb_grammar PRIVATE -fvisibility=hidden)
# Keep 16 KB page alignment for Android 15+ devices.
target_link_options(minkb_grammar PRIVATE "-Wl,-z,max-page-size=16384")
