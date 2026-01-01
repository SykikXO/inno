# Maintainer: Sykik <sykik@example.com>
pkgname=inno
pkgver=0.1
pkgrel=1
pkgdesc="A lightweight, event-driven Wayland notification agent"
arch=('x86_64')
url="https://github.com/sykik/dum"
license=('MIT')
depends=('wayland' 'cairo' 'dbus' 'glibc')
makedepends=('cmake' 'make' 'wayland-protocols')
source=()
md5sums=()

build() {
  cmake -B build -S "$srcdir/.." \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX=/usr
  cmake --build build
}

package() {
  DESTDIR="$pkgdir" cmake --install build
}
