#!/bin/bash
set -euo pipefail

# ===== CONFIGURATION =====
CORES=10
SYSROOT="/tmp/seiryotmp"
SRC_DIR="$HOME/.tmp/seiryobuild"
BUILD_DIR="$SRC_DIR/build"
PKG_DIR="$SRC_DIR/packages"
# Flavour (for .kpkg metadata)
FLAVOUR="glibc-systemd"
ARCH="x86_64"

# Create dirs
mkdir -p "$SRC_DIR" "$BUILD_DIR" "$PKG_DIR" "$SYSROOT"

# ===== HELPER FUNCTIONS =====
download() {
    local name=$1 version=$2 url=$3
    local src_dir="$SRC_DIR/$name-$version"
    if [[ -d "$src_dir" ]]; then
        echo "⏩ $name $version already downloaded"
        return 0
    fi
    echo "📥 Downloading $name $version..."
    wget -q -O "$name-$version.tar.xz" "$url"
    tar -xf "$name-$version.tar.xz" -C "$SRC_DIR"
    rm "$name-$version.tar.xz"
}

build_package() {
    local name=$1 version=$2 configure_args=("${@:3}")
    local src_dir="$SRC_DIR/$name-$version"
    local build_dir="$BUILD_DIR/$name-$version"
    local pkg_dir="$PKG_DIR/$name"

    # Skip if .kpkg exists
    if [[ -f "$PKG_DIR/${name}-${version}-${ARCH}.kpkg" ]]; then
        echo "✅ $name $version already built"
        return 0
    fi

    echo "🔧 Building $name $version..."
    mkdir -p "$build_dir"
    cd "$build_dir"
# Ensure config.guess and config.sub are available
    if [ ! -f config.guess ]; then
 	echo "📥 Fetching config.guess and config.sub..."
    	wget -O config.guess 'https://git.savannah.gnu.org/cgit/config.git/plain/config.guess'
    	wget -O config.sub 'https://git.savannah.gnu.org/cgit/config.git/plain/config.sub'
    	chmod +x config.guess config.sub
    fi
    # Configure
    "$src_dir/configure" \
    	--prefix=/usr \
    	--build=$(./config.guess) \
    	--host=x86_64-unknown-linux-gnu \
    	--target=x86_64-unknown-linux-gnu \
    	--disable-gprofng \
    # Build
    make -j$CORES

    # Install to staging
    make DESTDIR="$build_dir/staging" install
    # In binutils package staging dir
    # Create package layout
    mkdir -p "$pkg_dir/files"
    cp -a "$build_dir/staging/"* "$pkg_dir/files/"
    # In binutils package staging dir
    cd "$pkg_dir/files/usr/bin"

# Create prefixed symlinks
    # Create package.kdl
    cat > "$pkg_dir/package.kdl" <<EOF
package "$name" version="$version" arch="$ARCH" flavour="$FLAVOUR" {
  depends "glibc"
  license "GPL-2.0-or-later"
}
EOF

    # Build .kpkg
    kspkg buildpkg "$pkg_dir"

    # Move to packages dir
    mv "$pkg_dir/${name}-${version}-${ARCH}.kpkg" "$PKG_DIR/"

    echo "📦 $name $version packaged"
}

# ===== PACKAGE LIST (in build order) =====

# 1. linux-headers
download linux 6.12 https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.12.tar.xz
if [[ ! -f "$PKG_DIR/linux-headers-6.12-${ARCH}.kpkg" ]]; then
    echo "🔧 Building linux-headers..."
    cd "$SRC_DIR/linux-6.12"
    make headers_install INSTALL_HDR_PATH="$SYSROOT"
    mkdir -p "$PKG_DIR/linux-headers/files/usr"
    cp -a "$SYSROOT/include" "$PKG_DIR/linux-headers/files/usr/"
    cat > "$PKG_DIR/linux-headers/package.kdl" <<EOF
package "linux-headers" version="6.12" arch="$ARCH" flavour="$FLAVOUR" {
  depends "glibc"
  license "GPL-2.0"
}
EOF
    kspkg buildpkg "$PKG_DIR/linux-headers"
    mv "$PKG_DIR/linux-headers/linux-headers-6.12-${ARCH}.kpkg" "$PKG_DIR/"
    echo "📦 linux-headers packaged"
fi

# 2. glibc
download glibc 2.40 https://ftp.gnu.org/gnu/glibc/glibc-2.40.tar.xz
build_package glibc 2.40 \
    --with-headers="$SYSROOT/include" \
    --enable-kernel=6.6 \
    --disable-profile

# 3. binutils
download binutils 2.43 https://ftp.gnu.org/gnu/binutils/binutils-2.43.tar.xz
build_package binutils 2.43 \
    --enable-gold=yes \
    --enable-ld=default \
    --enable-plugins

# 4. gcc (stage 1: just C compiler)
download gcc 14.2.0 https://ftp.gnu.org/gnu/gcc/gcc-14.2.0/gcc-14.2.0.tar.xz
if [[ ! -f "$PKG_DIR/gcc-14.2.0-${ARCH}.kpkg" ]]; then
    echo "🔧 Building gcc (stage 1)..."
    mkdir -p "$BUILD_DIR/gcc-14.2.0"
    cd "$BUILD_DIR/gcc-14.2.0"
    "$SRC_DIR/gcc-14.2.0/configure" \
        --prefix=/usr \
        --host=x86_64-unknown-linux-gnu \
	--target=x86_64-unknown-linux-gnu \
        --build=$(sh "$SRC_DIR/gcc-14.2.0/config.guess") \
        --enable-languages=c,c++ \
        --disable-multilib \
        --disable-bootstrap \
        --with-system-zlib \
    	--enable-targets=all \
	--with-sysroot=/tmp/seiryoroot \ 
    	--disable-werror
    make -j$CORES all-gcc all-target-libgcc
    make DESTDIR="$BUILD_DIR/gcc-14.2.0/staging" install-gcc install-target-libgcc

    mkdir -p "$PKG_DIR/gcc/files"
    cp -a "$BUILD_DIR/gcc-14.2.0/staging/"* "$PKG_DIR/gcc/files/"
    cat > "$PKG_DIR/gcc/package.kdl" <<EOF
package "gcc" version="14.2.0" arch="$ARCH" flavour="$FLAVOUR" {
  depends "glibc"
  depends "binutils"
  license "GPL-3.0"
}
EOF
    kspkg buildpkg "$PKG_DIR/gcc"
    mv "$PKG_DIR/gcc/gcc-14.2.0-${ARCH}.kpkg" "$PKG_DIR/"
    echo "📦 gcc packaged"
fi

# 5. zlib
download zlib 1.3.1 https://zlib.net/zlib-1.3.1.tar.gz
(
    cd "$SRC_DIR/zlib-1.3.1"
    # Prevent configure from aborting on warnings
    mkdir -p "$PKG_DIR/zlib/files"
    cp -a "$BUILD_DIR/zlib-staging/"* "$PKG_DIR/zlib/files/"
    cat > "$PKG_DIR/zlib/package.kdl" <<EOF
package "zlib" version="1.3.1" arch="$ARCH" flavour="$FLAVOUR" {
  depends "glibc"
  license "Zlib"
}
EOF
    kspkg buildpkg "$PKG_DIR/zlib"
    mv "$PKG_DIR/zlib/zlib-1.3.1-${ARCH}.kpkg" "$PKG_DIR/"
)

# 6. coreutils
download coreutils 9.5 https://ftp.gnu.org/gnu/coreutils/coreutils-9.5.tar.xz
build_package coreutils 9.5 \
    --enable-no-install-program=stdbuf,libstdbuf \
    --without-gmp

# Download zsh
download zsh 5.9 https://www.zsh.org/pub/zsh-5.9.tar.xz

# Build zsh
if [[ ! -f "$PKG_DIR/zsh-5.9-${ARCH}.kpkg" ]]; then
    echo "🔧 Building zsh 5.9..."
    mkdir -p "$BUILD_DIR/zsh-5.9"
    cd "$BUILD_DIR/zsh-5.9"

    # Configure with sysroot + host triplet
    CC=x86_64-unknown-linux-gnu-gcc \
    CFLAGS="--sysroot=$SYSROOT -O2" \
    LDFLAGS="--sysroot=$SYSROOT" \
    sh "$SRC_DIR/zsh-5.9/configure" \
        --prefix=/usr \
        --host=x86_64-unknown-linux-gnu \
        --enable-static \
        --disable-dynamic \
        --disable-gdbm \
        --with-term-lib=ncurses

    make -j$CORES

    # Install
    make DESTDIR="$BUILD_DIR/zsh-staging" install

    # Package
    mkdir -p "$PKG_DIR/zsh/files"
    cp -a "$BUILD_DIR/zsh-staging/"* "$PKG_DIR/zsh/files/"

    cat > "$PKG_DIR/zsh/package.kdl" <<EOF
package "zsh" version="5.9" arch="$ARCH" flavour="$FLAVOUR" {
  depends "glibc"
  depends "ncurses"
  license "MIT"
}
EOF

    kspkg buildpkg "$PKG_DIR/zsh"
    mv "$PKG_DIR/zsh/zsh-5.9-${ARCH}.kpkg" "$PKG_DIR/"
    echo "📦 zsh packaged"
fi

# 8. util-linux
download util-linux 2.40 https://www.kernel.org/pub/linux/utils/util-linux/v2.40/util-linux-2.40.tar.xz
build_package util-linux 2.40 \
    --disable-chfn-chsh \
    --disable-login \
    --disable-nologin \
    --disable-su \
    --disable-sulogin \
    --disable-pylibmount

echo "🎉 All core packages built and packaged!"
echo "📦 Packages in: $PKG_DIR"
