@echo off
setlocal enabledelayedexpansion

:: Unset old CUDA environment variables
set CUDA_PATH_V11_3=
set CUDA_PATH_V11_7=
set CUDA_PATH_V11_8=

:: Setup CUDA 13.2 PATH FIRST (before other CUDA versions)
set CUDA_PATH=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.2
set CUDACXX=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.2\bin\nvcc.exe
set PATH=C:\Users\silve\ninja;%CUDA_PATH%\bin;%PATH%

:: Setup Visual Studio environment
call "C:\Program Files\Microsoft Visual Studio\18\Community\Common7\Tools\VsDevCmd.bat" -arch=amd64

:: Setup CMake
set CMAKE_GENERATOR=Ninja
set CMAKE_MAKE_PROGRAM=C:\Users\silve\ninja\ninja.exe
set CMAKE_CUDA_FLAGS=--allow-unsupported-compiler

:: Build
cargo build --release