#!/bin/sh
# CGO compiler wrapper for an iOS device c-archive (arm64 / iphoneos).
SDK=iphoneos
PLATFORM=ios
CLANGARCH=arm64
MIN=17.0
SDK_PATH=$(xcrun --sdk "$SDK" --show-sdk-path)
CLANG=$(xcrun --sdk "$SDK" --find clang)
exec "$CLANG" -arch "$CLANGARCH" -isysroot "$SDK_PATH" -m${PLATFORM}-version-min=$MIN "$@"
