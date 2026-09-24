# The upstream libdatachannel build does not set ANDROID_ABI from Rust's target.
# Pin all Android C/C++ dependencies to the same ABI and API level as the .so.
if(DEFINED ENV{AI_TERMINAL_ANDROID_ABI})
    set(ANDROID_ABI "$ENV{AI_TERMINAL_ANDROID_ABI}" CACHE STRING "" FORCE)
else()
    set(ANDROID_ABI "arm64-v8a" CACHE STRING "" FORCE)
endif()
set(ANDROID_PLATFORM "android-25" CACHE STRING "" FORCE)
set(ANDROID_STL "c++_shared" CACHE STRING "" FORCE)
include("$ENV{ANDROID_NDK_HOME}/build/cmake/android.toolchain.cmake")
