# Desktop build of the keyboard's grammar engine, for testing prompts and models:
#   cmake -S tools/grammar_cli -B build/grammar_cli -DCMAKE_BUILD_TYPE=Release
#   cmake --build build/grammar_cli -j
#   build/grammar_cli/grammar_cli model.gguf English "She go to school yesterday."
cmake_minimum_required(VERSION 3.22.1)
project(grammar_cli CXX C)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CORE_DIR ${CMAKE_CURRENT_LIST_DIR}/../../android/app/src/main/cpp)

include(${CORE_DIR}/llama.cmake)

add_executable(grammar_cli main.cpp ${CORE_DIR}/grammar_core.cpp)
target_include_directories(grammar_cli PRIVATE ${CORE_DIR})
target_link_libraries(grammar_cli PRIVATE llama)
