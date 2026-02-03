# Maintainer: Sykik <sykik@example.com>
pkgname=inno
pkgver=0.2.0
pkgrel=1
pkgdesc="A lightweight,event-driven Wayland notification agent (Rust)"
arch=('x86_64')
url="https://github.com/sykik/dum"
license=('MIT')
depends=('wayland' 'cairo' 'dbus' 'glibc')
makedepends=('rust' 'cargo')
source=()
md5sums=()

build() {
  cd "$srcdir/.."
  cargo build --release
}

package() {
  cd "$srcdir/.."
  install -Dm755 target/release/inno "$pkgdir/usr/bin/inno"
}
